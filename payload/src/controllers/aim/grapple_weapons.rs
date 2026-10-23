//! The grapple's aim sees weapons: the scoped patch of the grapple snatch
//! (`docs/mod/input/grapple-snatch.md`, "Targeting"), with the engine side in "Weapons and the
//! grapple" in `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! The direct-aim ray's query layer never meets a weapon's body, and no query layer is that layer's
//! set plus the weapons'. So inside the grapple's pass of the direct-aim raycast
//! ([`crate::controllers::aim::grapple`]), and only there, the distribution of the ray's hits
//! ([`PlayerAimControl::CheckDirectTargets`]) is preceded by a second cast along the same ray, on a
//! layer that meets weapons, out to the ray's own hit. A weapon lying in the world that it finds is
//! written into the ray's closest hit, so the game does everything it does with a hit with it: the
//! slot's game object, the grapplable flag, the last-valid grapple cache, and the latch of the
//! fire. The camera's pass, the gun's aim, and every other query see the world as before.
//!
//! The weapon found, or else the object the aim's own ray hit, is published ([`aimed`]) for the
//! snatch's targeting.

use std::{
    cell::Cell,
    num::NonZeroUsize,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
    aim::aim::{PlayerAimControl, PlayerAimRaycastCollector},
    equipment::weapon::{WeaponBase, WeaponState},
    game::GameObject,
    physics::{CastRayInfo, PfxLayer, PhysicsGameObject},
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions, RayHit},
    },
    weapons::hold::refs::{Strong, Weak},
};

/// The patch's hooks, aggregated by [`crate::controllers::aim::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CHECK_DIRECT_TARGETS_BINDER)
}

/// The object under the grapple's aim: a weapon lying in the world the weapon cast found, or else
/// the object the aim's own ray hit.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Aimed {
    /// The object (its physics game object: a `CWeaponBase` for a weapon).
    pub object: NonZeroUsize,
    /// Whether it is a weapon lying in the world.
    pub weapon: bool,
    /// The body the ray hit.
    pub body: u32,
    /// Where the ray hit it, world space.
    pub point: Vec3,
    /// The surface normal there, toward the ray's origin.
    pub normal: Vec3,
    /// How far along the ray, metres.
    pub distance: f32,
}

/// Turn the patch on or off. The snatch's input tick sets it from its switch.
pub(crate) fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    if !enabled {
        *AIMED.lock() = None;
    }
}

/// The object under the grapple's aim at the last aim update, with a strong reference that keeps
/// it alive while held, when it still lives and the update was recent.
pub(crate) fn aimed() -> Option<(Aimed, Strong)> {
    let aimed = AIMED.lock();
    let (weapon, weak, at) = aimed.as_ref()?;
    if at.elapsed() > AIMED_FOR {
        return None;
    }
    Some((*weapon, weak.lock()?))
}

/// Run `f` as the grapple's pass of the direct-aim raycast: the patch acts only inside it.
pub(super) fn in_grapple_pass<R>(f: impl FnOnce() -> R) -> R {
    let previous = GRAPPLE_PASS.with(|p| p.replace(true));
    let result = f();
    GRAPPLE_PASS.with(|p| p.set(previous));
    result
}

/// Whether the object a cast hit is a weapon lying in the world.
///
/// # Safety
///
/// `object` must be a live physics game object.
pub(crate) unsafe fn is_dropped_weapon(object: NonZeroUsize) -> bool {
    let game_object =
        (object.get() + PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize) as *const GameObject;
    if !unsafe { (*game_object).IsType(WeaponBase::TYPE_ID()) } {
        return false;
    }
    let weapon = object.get() as *const WeaponBase;
    // Read as the raw value: a state the enum does not name is not a dropped one.
    let state = unsafe { (&raw const (*weapon).m_State).cast::<i32>().read() };
    state == WeaponState::E_WS_DROPPED as i32
}

/// How long a published aim stays current.
const AIMED_FOR: Duration = Duration::from_millis(250);
/// The most hits the weapon cast steps past (a character's capsule, a projectile, a vehicle's
/// collision body) on its way to a weapon.
const MAX_STEPS: usize = 3;
/// How far past a hit the next step of the weapon cast starts, metres.
const STEP_PAST_M: f32 = 0.02;

static ENABLED: AtomicBool = AtomicBool::new(false);
static AIMED: Mutex<Option<(Aimed, Weak, Instant)>> = Mutex::new(None);

thread_local! {
    /// Whether this thread is in the grapple's pass of the direct-aim raycast.
    static GRAPPLE_PASS: Cell<bool> = const { Cell::new(false) };
}

/// The distribution of the direct-aim ray's hits, preceded in the grapple's pass by the weapon
/// cast (see the module docs).
#[detour(address = jc3gi::aim::aim::PlayerAimControl::CheckDirectTargets_ADDRESS)]
fn check_direct_targets(
    aim: *mut PlayerAimControl,
    collector: *const PlayerAimRaycastCollector,
    info: *mut CastRayInfo,
    ray_start: *const Vector3,
    ray_dir: *const Vector3,
) {
    if ENABLED.load(Ordering::Relaxed) && GRAPPLE_PASS.with(Cell::get) {
        // SAFETY: the engine's own arguments, in the aim update, on the thread casting the ray.
        unsafe { merge_weapon(aim, collector.cast_mut(), info, ray_start, ray_dir) };
    }
    CHECK_DIRECT_TARGETS
        .get()
        .unwrap()
        .call(aim, collector, info, ray_start, ray_dir)
}

/// Cast for a weapon along the ray out to its closest hit, and write the nearest one found into
/// that hit; publish it, or that there is none.
///
/// # Safety
///
/// The arguments must be the direct-aim raycast's, inside its update.
unsafe fn merge_weapon(
    aim: *mut PlayerAimControl,
    collector: *mut PlayerAimRaycastCollector,
    info: *mut CastRayInfo,
    ray_start: *const Vector3,
    ray_dir: *const Vector3,
) {
    let found = unsafe { find_weapon(aim, info, ray_start, ray_dir) };
    let Some((hit, object, length)) = found else {
        *AIMED.lock() = unsafe { aimed_hit(aim, info, ray_start, ray_dir) };
        return;
    };
    // SAFETY: checked non-null by the search, and the hit object is live during its cast.
    let info = unsafe { &mut *info };
    info.base.m_Normal = Vector3 {
        data: hit.normal.to_array(),
    };
    info.base.m_HitDistance = hit.distance / length;
    info.base.m_BodyId = hit.body;
    info.m_GameObject = object.get() as *mut _;
    // A holey hit beyond the weapon would win the slots that stop at holey geometry.
    if let Some(collector) = unsafe { collector.as_mut() }
        && collector.m_HasHoleyHit
        && collector.m_HoleyHitFraction * length > hit.distance
    {
        collector.m_HasHoleyHit = false;
    }
    let weapon = unsafe { &*(object.get() as *const WeaponBase) };
    // SAFETY: the weapon's own weak reference to itself, live while it is.
    let weak = unsafe { Weak::of(&weapon.m_WeakThis) };
    *AIMED.lock() = Some((
        Aimed {
            object,
            weapon: true,
            body: hit.body,
            point: hit.position,
            normal: hit.normal,
            distance: hit.distance,
        },
        weak,
        Instant::now(),
    ));
}

/// The object the aim's own ray hit, to publish, when it hit one.
///
/// # Safety
///
/// As [`merge_weapon`].
unsafe fn aimed_hit(
    aim: *mut PlayerAimControl,
    info: *mut CastRayInfo,
    ray_start: *const Vector3,
    ray_dir: *const Vector3,
) -> Option<(Aimed, Weak, Instant)> {
    let aim = unsafe { aim.as_ref() }?;
    let info = unsafe { info.as_ref() }?;
    let object = NonZeroUsize::new(info.m_GameObject as usize)?;
    let fraction = info.base.m_HitDistance;
    let length = aim.m_RayCastDistance;
    if !(0.0..1.0).contains(&fraction) || !(length.is_finite() && length > 0.0) {
        return None;
    }
    let start = Vec3::from_array(unsafe { ray_start.as_ref() }?.data);
    let direction = Vec3::from_array(unsafe { ray_dir.as_ref() }?.data);
    let distance = fraction * length;
    let game_object =
        (object.get() + PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize) as *const GameObject;
    // SAFETY: the hit object is live during its cast, and so is its weak reference to itself.
    let weak = unsafe { Weak::of(&(*game_object).m_WeakThis) };
    Some((
        Aimed {
            object,
            weapon: false,
            body: info.base.m_BodyId,
            point: start + direction * distance,
            normal: Vec3::from_array(info.base.m_Normal.data),
            distance,
        },
        weak,
        Instant::now(),
    ))
}

/// The nearest weapon lying in the world along the ray, short of its closest hit, with the ray's
/// length.
///
/// # Safety
///
/// As [`merge_weapon`].
unsafe fn find_weapon(
    aim: *mut PlayerAimControl,
    info: *mut CastRayInfo,
    ray_start: *const Vector3,
    ray_dir: *const Vector3,
) -> Option<(RayHit, NonZeroUsize, f32)> {
    let aim = unsafe { aim.as_ref() }?;
    let info = unsafe { info.as_ref() }?;
    let start = Vec3::from_array(unsafe { ray_start.as_ref() }?.data);
    let direction = Vec3::from_array(unsafe { ray_dir.as_ref() }?.data);
    let length = aim.m_RayCastDistance;
    if !(length.is_finite() && length > 0.0) {
        return None;
    }
    let fraction = info.base.m_HitDistance;
    let reach = if (0.0..1.0).contains(&fraction) {
        fraction * length
    } else {
        length
    };
    let owner = unsafe { aim.m_Owner.as_ref() }?;
    let vehicle = unsafe { owner.GetVehiclePtr() } as usize;
    let options = CastOptions {
        ignore: [owner as *const _ as usize, vehicle],
        see_through: true,
        material: false,
    };
    let mut from = 0.0;
    for _ in 0..MAX_STEPS {
        let remaining = reach - from;
        if remaining <= 0.0 {
            return None;
        }
        let ray = Ray::new(start + direction * from, direction)?;
        // SAFETY: the aim update, in the simulation phase, outside the physics step.
        let hit = unsafe {
            raycast::cast_on(&ray, remaining, &options, PfxLayer::VEHICLE_RAYCAST as i32)
        }
        .ok()??;
        let hit = RayHit {
            distance: hit.distance + from,
            ..hit
        };
        if let Some(object) = hit.object
            && unsafe { is_dropped_weapon(object) }
        {
            return Some((hit, object, length));
        }
        from = hit.distance + STEP_PAST_M;
    }
    None
}
