//! Bullets through a pane the glass has brought down.
//!
//! Hiding a pane ([`engine::hide`](crate::controllers::glass::engine::hide)) takes away only its
//! drawing: its shapes still answer ray queries with the glass material, so a bullet still meets
//! glass where the pane was. The bullet's hit handling would then crack the hidden pane, deal the
//! hit to the vehicle as a hit on it, and stop the bullet unless a roll against the glass
//! material's collide factor lets it through ([`Bullet::HandleHit`]).
//!
//! So a hit on glass whose pane the glass brought down, and which is still hidden, is passed
//! through instead, as the bullet's own penetration passes it: the bullet moves to the hit, its next
//! ray starts there, and the hit object becomes the one object its ray filter ignores
//! ([`Bullet::m_HasPenetratedOneObject`]). Nothing else of the hit runs: no crack, no damage or
//! impulse to the vehicle, no hit effects, and for that one segment no check against creatures or
//! the water surface. A hit on world glass, which has no object to ignore, is left to the game.
//!
//! The bullets update on worker threads, so the handler runs on one: it reads the hit, the panes
//! the hit names (as the bullet's glass handling looks them up there), and the set of panes the
//! glass brought down, under its own lock.
//!
//! The first glass hit logs the glass material's collide factor, how often intact glass stops a
//! bullet in the game's data.

use std::{
    mem::MaybeUninit,
    ptr,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
    equipment::weapon::Bullet,
    game::{FindOptionalInternal, GameObject},
    graphics_engine::model_instance::{ModelInstance, ModelInstanceArray, ModelInstanceFlags},
    physics::{
        CastRayInfo, CastRayInfoBase, GameMaterialId, GetModelInstancesHit, PfxCharacterInstance,
        PhysicsGameObject, PhysicsSystem,
    },
    types::{
        math::Vector3,
        shared_ptr::{ReleaseSharedPtrGameObject, SharedPtr},
    },
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::config::CONTROLLERS_CONFIG;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&HANDLE_HIT_BINDER)
}

/// How many bullets passed through a pane the glass brought down, for the debug UI.
pub fn passes() -> u64 {
    PASSES.load(Ordering::Relaxed)
}

/// The glass material's collide factor, once a bullet has met glass: the chance a bullet stops at
/// intact glass.
pub fn collide_factor() -> Option<f32> {
    *COLLIDE_FACTOR.lock()
}

/// Whether the glass has brought any pane down since the game started, read without the lock.
pub(crate) fn any_hidden() -> bool {
    ANY_HIDDEN.load(Ordering::Relaxed)
}

/// Note the pane `instance` as brought down by the glass, so bullets pass through it.
pub(super) fn note_hidden(instance: *mut ModelInstance) {
    let mut hidden = HIDDEN.lock();
    let address = instance as usize;
    if !hidden.contains(&address) {
        if hidden.len() >= MAX_HIDDEN {
            hidden.remove(0);
        }
        hidden.push(address);
    }
    ANY_HIDDEN.store(true, Ordering::Relaxed);
}

#[detour(address = jc3gi::equipment::weapon::Bullet::HandleHit_ADDRESS)]
fn handle_hit(bullet: *mut Bullet, info: *mut CastRayInfoBase, dt: f32) {
    // SAFETY: the bullet's own hit, on the worker updating it.
    if unsafe { pass_through(bullet, info) } {
        return;
    }
    HANDLE_HIT.get().unwrap().call(bullet, info, dt);
}

/// Pass `bullet` through its hit `info` when the hit is on a pane the glass brought down. Returns
/// whether it did, in which case the hit is handled.
///
/// # Safety
///
/// The bullet's hit handling's own arguments, on the worker updating it.
unsafe fn pass_through(bullet: *mut Bullet, info: *mut CastRayInfoBase) -> bool {
    let (Some(bullet), Some(info)) = (unsafe { bullet.as_mut() }, unsafe { info.as_mut() }) else {
        return false;
    };
    let hit_fraction = info.m_HitDistance;
    if hit_fraction.is_nan()
        || hit_fraction >= 1.0
        || info.m_BodyId == PfxCharacterInstance::INVALID_BODY_ID
    {
        return false;
    }
    // The material is resolved from the base's hit fields and the physics world alone, whatever
    // the info's own class.
    let material = unsafe { (*ptr::from_mut(info).cast::<CastRayInfo>()).GetHitMaterial() };
    if material != GameMaterialId::EGameMaterialId_Glass as i32 {
        return false;
    }
    note_collide_factor();
    if !ANY_HIDDEN.load(Ordering::Relaxed) || !enabled() || info.m_HitObjectId == 0 {
        return false;
    }
    if !unsafe { names_hidden_pane(info) } {
        return false;
    }
    let Some(object) = (unsafe { physics_object(info.m_HitObjectId) }) else {
        return false;
    };
    let source = Vec3::from_array(bullet.m_RaySource.data);
    let target = Vec3::from_array(bullet.m_RayTarget.data);
    let along = info.m_HitDistance * source.distance(target);
    let hit = Vec3::from_array(bullet.m_PreviousPosition.data)
        + Vec3::from_array(bullet.m_Direction.data) * along;
    if !hit.is_finite() {
        return false;
    }
    let hit = Vector3 {
        data: hit.to_array(),
    };
    bullet.m_Position = hit;
    bullet.m_PreviousPosition = hit;
    bullet.m_HasPenetratedOneObject = true;
    bullet.m_RayIgnoreList = [object, ptr::null_mut()];
    if PASSES.fetch_add(1, Ordering::Relaxed) == 0 {
        tracing::info!(
            target: "controllers",
            "glass: a bullet passed through a pane brought down"
        );
    }
    true
}

/// Whether the glass's switches let bullets pass through the panes it brought down.
fn enabled() -> bool {
    CONTROLLERS_CONFIG.query(|c| {
        let glass = &c.glass;
        glass.enabled && glass.shatter && glass.bullets_pass
    })
}

/// Whether the hit `info` names a pane the glass brought down that is still hidden.
///
/// # Safety
///
/// A current physics hit, on the simulation thread outside the physics step or its query worker.
pub(crate) unsafe fn names_hidden_pane(info: &mut CastRayInfoBase) -> bool {
    let mut found = [ptr::null_mut::<ModelInstance>(); MAX_PANES];
    let base = found.as_mut_ptr();
    // Every field is a pointer or an integer, so all-zero is a valid value to fill in.
    // SAFETY: as above.
    let mut array = unsafe { MaybeUninit::<ModelInstanceArray>::zeroed().assume_init() };
    array.m_Begin = base;
    array.m_End = base;
    // SAFETY: one past the end of the buffer.
    array.m_Last = unsafe { base.add(MAX_PANES) };
    array.m_Flags = ModelInstanceArray::USER_ALLOCATED;
    // SAFETY: as the bullet's glass handling calls it, from the bullet's worker.
    unsafe { GetModelInstancesHit(info, &mut array, ptr::null_mut()) };
    // SAFETY: the call appends within the buffer.
    let count = usize::try_from(unsafe { array.m_End.offset_from(array.m_Begin) }).unwrap_or(0);
    let hidden = HIDDEN.lock();
    found[..count.min(MAX_PANES)].iter().any(|&instance| {
        !instance.is_null()
            && hidden.contains(&(instance as usize))
            // SAFETY: an instance the engine just named is live.
            && !unsafe { (*instance).m_Flags }.contains(ModelInstanceFlags::VISIBLE)
    })
}

/// The physics game object the object id `id` names, as the bullet's ray filter ignores it.
///
/// # Safety
///
/// The bullet's worker, as the bullet's hit handling looks its hit object up.
unsafe fn physics_object(id: u64) -> Option<*mut core::ffi::c_void> {
    let mut found = SharedPtr::<GameObject> {
        px: ptr::null_mut(),
        pn: ptr::null_mut(),
    };
    // SAFETY: a plain id and a reference the call fills.
    unsafe { FindOptionalInternal(id, &mut found) };
    let object = found.px;
    // SAFETY: the reference keeps the object alive for the type check.
    let physics = (!object.is_null() && unsafe { (*object).IsType(PhysicsGameObject::TYPE_ID()) })
        .then(|| {
            (object as usize - PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize)
                as *mut core::ffi::c_void
        });
    if !found.pn.is_null() {
        // SAFETY: the strong reference the lookup took. The world keeps its own, so the object
        // outlives this release, as it outlives the bullet's.
        unsafe { ReleaseSharedPtrGameObject(&mut found) };
    }
    physics
}

/// Log the glass material's collide factor, once.
fn note_collide_factor() {
    if NOTED.swap(true, Ordering::Relaxed) {
        return;
    }
    // SAFETY: the material table is loaded with the physics system and only read afterwards.
    let factor = unsafe { PhysicsSystem::get() }
        .and_then(|physics| unsafe { physics.m_MaterialManager.as_ref() })
        .and_then(|materials| unsafe {
            materials
                .GetMaterialDesc(GameMaterialId::EGameMaterialId_Glass)
                .as_ref()
        })
        .map(|desc| desc.m_BulletCollideFactor);
    *COLLIDE_FACTOR.lock() = factor;
    tracing::info!(
        target: "controllers",
        collide_factor = ?factor,
        "glass: the glass material's bullet collide factor (the chance intact glass stops a bullet)"
    );
}

/// The panes the glass brought down, by instance address.
static HIDDEN: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Whether any pane has been brought down, read without the lock on every glass hit.
static ANY_HIDDEN: AtomicBool = AtomicBool::new(false);

static PASSES: AtomicU64 = AtomicU64::new(0);

static NOTED: AtomicBool = AtomicBool::new(false);

static COLLIDE_FACTOR: Mutex<Option<f32>> = Mutex::new(None);

/// The most panes one hit is looked up for, as many as a bullet looks up.
const MAX_PANES: usize = 20;

/// The most brought-down panes remembered; the oldest is forgotten past it.
const MAX_HIDDEN: usize = 256;
