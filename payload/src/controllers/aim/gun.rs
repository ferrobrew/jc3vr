//! Gun aim from the hands: the plan of record's "Guns" (`docs/mod/input/controllers-and-roomscale.md`),
//! with the engine side in `docs/engine/gameplay/aim-pipeline.md`.
//!
//! Each gun of the player's wielded weapon aims along its own sight line
//! ([`ray::sight_ray`]): the gun's transform carried to the controller's current pose by the hand's
//! correction (as the render-time late update carries the rendered gun), composed with the gun's
//! current fire point and the weapon's sight correction. The line is cast once per aim update, in
//! the simulation phase, and its hit (or a far point along it, never nearer than the configured
//! minimum) becomes that hand's aim point.
//!
//! The aim point reaches the weapon at the per-weapon update, the one seam that gives two guns two
//! directions: the base update ([`update_from_user_post_sim`]) is handed the hand's point in place
//! of the character's shared one, for the gun that will be updated and can fire in that call
//! ([`WeaponBase::m_NextWeaponToFire`]). The weapon stores it as its own aim target before it
//! builds the shot, and every weapon class's update runs through the base. For the player, the aim
//! update and the weapon updates run one after the other on the same thread, aim first, so the
//! points are always the current tick's.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::weapon::{InternalFireInformation, WeaponBase, WeaponData, WeaponState},
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::{
        TickHands,
        ray::{self, Ray},
    },
    config::AimConfig,
    frame::Hand,
    hands::late,
};

/// The gun hooks, aggregated by [`crate::controllers::aim::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_FROM_USER_POST_SIM_BINDER)
}

/// The player's wielded weapon, as the gun aim sees it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeldWeapon {
    /// The weapon's address.
    pub address: usize,
    /// The weapon type's name hash, the key of the sight table.
    pub hash: u32,
    /// The hand each gun rides, indexed by gun; `None` for a gun off the hands.
    pub gun_hands: [Option<Hand>; MAX_GUNS],
    /// Whether the sight table corrects this weapon.
    pub sight_corrected: bool,
}

/// The most guns per weapon the gun aim tracks. A dual-wield weapon has two.
pub const MAX_GUNS: usize = 4;

/// The per-weapon update's substitutions since the payload loaded, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct GunCounters {
    /// Calls handed a hand's aim point.
    pub aimed: u64,
    /// Calls for the aimed weapon that kept the game's point: the gun about to fire rides no
    /// tracked hand.
    pub kept: u64,
}

pub fn counters() -> GunCounters {
    GunCounters {
        aimed: AIMED.load(Ordering::Relaxed),
        kept: KEPT.load(Ordering::Relaxed),
    }
}

/// The player's wielded weapon, when it is held in the hands: the selected slot's weapon, used by
/// `owner`, wielded or dual-wielded.
///
/// # Safety
///
/// `owner` must be the live local player character.
pub(crate) unsafe fn held_weapon(owner: &Character) -> Option<&'static WeaponBase> {
    // Read as the raw value: the slot can be out of the enum's range, and then indexes nothing.
    let slot = unsafe { (&raw const owner.m_SelectedWeaponSlot).cast::<i32>().read() };
    let slot = usize::try_from(slot).ok()?;
    let weapon = unsafe { owner.m_Inventory.m_Weapons.get(slot)?.px.as_ref()? };
    let owned = std::ptr::eq(weapon.m_User[0].cast::<Character>(), owner);
    let wielded = matches!(
        weapon.m_State,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    );
    (owned && wielded).then_some(weapon)
}

/// The sight line of each hand that holds a gun of `weapon`, and the weapon's readout. A hand
/// holding two guns takes the first.
///
/// # Safety
///
/// `weapon` must be live, called from the simulation thread.
pub(super) unsafe fn sight_rays(
    weapon: &WeaponBase,
    hands: &TickHands,
    config: &AimConfig,
) -> (HeldWeapon, [Option<(usize, Ray)>; 2]) {
    let sight = config.sights.get(weapon.m_WeaponNameHash);
    let mut held = HeldWeapon {
        address: weapon as *const WeaponBase as usize,
        hash: weapon.m_WeaponNameHash,
        gun_hands: [None; MAX_GUNS],
        sight_corrected: sight.is_some(),
    };
    let mut rays = [None; 2];
    for (index, gun) in unsafe { guns(weapon) }.take(MAX_GUNS).enumerate() {
        let Some(gun) = gun else {
            continue;
        };
        let Some(&bone) = gun
            .m_lBoneAttachementPerWeaponState
            .get(weapon.m_State as usize)
        else {
            continue;
        };
        let Some(hand) = late::attach_hand(bone) else {
            continue;
        };
        held.gun_hands[index] = Some(hand);
        if rays[hand.index()].is_some() {
            continue;
        }
        let correction = hands.hands[hand.index()].and_then(|h| h.correction);
        let world = ray::corrected_gun(Mat4::from(gun.m_WorldMatrixT1), correction);
        let fire_point = unsafe { fire_point(gun) };
        rays[hand.index()] = ray::sight_ray(world, fire_point, sight).map(|r| (index, r));
    }
    (held, rays)
}

/// Publish the tick's per-gun aim points for the per-weapon update.
pub(super) fn publish(held: Option<HeldWeapon>, targets: [Option<Vec3>; 2]) {
    *SNAPSHOT.lock() = held.map(|weapon| Snapshot {
        weapon,
        targets,
        at: Instant::now(),
    });
}

/// The dominant hand's current aim point, for the character-level aim point: every other reader
/// of the player's weapon aim (weapons the per-weapon update is not handed a point for, and the
/// character's shared aim state) follows the gun hand too. `None` when the gun aim is not acting.
pub fn character_aim_point(dominant: Hand) -> Option<Vec3> {
    SNAPSHOT
        .lock()
        .filter(Snapshot::fresh)
        .and_then(|s| s.targets[dominant.index()])
}

/// The weapons' per-tick update from their user, handed the aiming hand's point for the player's
/// wielded weapon.
#[detour(address = jc3gi::equipment::weapon::WeaponBase::UpdateFromUserPostSim_ParallelBase_ADDRESS)]
fn update_from_user_post_sim(
    weapon: *mut WeaponBase,
    dt: f32,
    aim_position: *const Vector3,
    input: *const c_void,
) {
    let original = |aim_position: *const Vector3| {
        UPDATE_FROM_USER_POST_SIM
            .get()
            .unwrap()
            .call(weapon, dt, aim_position, input)
    };
    match unsafe { weapon.as_ref() }.and_then(hand_target) {
        Some(target) => {
            let target = Vector3 {
                data: target.to_array(),
            };
            original(&target);
        }
        None => original(aim_position),
    }
}

/// The aim point for the gun `weapon` will update next, when `weapon` is the aimed weapon.
fn hand_target(weapon: &WeaponBase) -> Option<Vec3> {
    let snapshot = SNAPSHOT.lock().filter(Snapshot::fresh)?;
    if snapshot.weapon.address != weapon as *const WeaponBase as usize {
        return None;
    }
    let target = snapshot
        .weapon
        .gun_hands
        .get(weapon.m_NextWeaponToFire as usize)
        .copied()
        .flatten()
        .and_then(|hand| snapshot.targets[hand.index()]);
    let counter = if target.is_some() { &AIMED } else { &KEPT };
    counter.fetch_add(1, Ordering::Relaxed);
    target
}

/// The tick's aim points, published by the aim update and read by the per-weapon update.
#[derive(Debug, Copy, Clone)]
struct Snapshot {
    weapon: HeldWeapon,
    targets: [Option<Vec3>; 2],
    at: Instant,
}

impl Snapshot {
    /// Whether the snapshot is from a recent aim update. The aim update runs every simulation tick
    /// while the player exists, so an older one belongs to a paused or departed player.
    fn fresh(&self) -> bool {
        self.at.elapsed() < MAX_AGE
    }
}

const MAX_AGE: Duration = Duration::from_millis(250);

static SNAPSHOT: Mutex<Option<Snapshot>> = Mutex::new(None);
static AIMED: AtomicU64 = AtomicU64::new(0);
static KEPT: AtomicU64 = AtomicU64::new(0);

/// The weapon's guns, in order.
pub(crate) unsafe fn guns(weapon: &WeaponBase) -> impl Iterator<Item = Option<&WeaponData>> {
    let list = &weapon.m_lWeaponData;
    let count = if list.m_Begin.is_null() || list.m_End < list.m_Begin {
        0
    } else {
        unsafe { list.m_End.offset_from(list.m_Begin) as usize }
    };
    (0..count).map(move |i| unsafe { (*list.m_Begin.add(i)).as_ref() })
}

/// The gun's current fire point in its own frame, or the identity when the cursor is out of range,
/// as the game treats it.
pub(crate) unsafe fn fire_point(gun: &WeaponData) -> Mat4 {
    let points = &gun.m_InternalFirePositions;
    let cursor = gun.m_InternalFirePositionCurrentIndex;
    usize::try_from(cursor)
        .ok()
        .filter(|&i| i < points.len())
        .map_or(Mat4::IDENTITY, |i| {
            let point: &InternalFireInformation = unsafe { &*points.begin.add(i) };
            Mat4::from(point.m_Position)
        })
}
