//! The per-hand tick: every gun a hand holds fires from that hand's own trigger and aims at that
//! hand's own point. The design is the two-handed inventory's "Firing"
//! (`docs/mod/input/two-handed-inventory.md`), the engine side in
//! `docs/engine/gameplay/weapon-firing.md`.
//!
//! The fire path keys everything on the weapon's user rather than its state or the selected slot,
//! so a weapon armed in a hand ([`crate::controllers::weapons::hold::armed`]) fires as the player's
//! from the holstered state. Each input tick publishes each hand's fire input ([`publish_fire`]),
//! and each aim update each hand's aim point ([`publish_targets`]). Then:
//!
//! - after the inventory's own weapon updates ([`Inventory::UpdateWeapons_Parallel`] and
//!   [`Inventory::UpdateWeapons_Serial`]), the mod updates each armed weapon the same way, through
//!   its [`WeaponBase::UpdateFromUserPostSim_Parallel`] and `_Serial` virtuals, with its hand's
//!   fire input and aim point. The shooting permission gates only the input the inventory's own
//!   fan-out passes down, so it is checked here for the mod's own
//!   ([`Inventory::GetAnimationAllowsShooting`]), as is the true reload's hold on the gun;
//! - with `weapons.hold.per_hand_tick` (on), the weapon the game wields, which the fan-out updates,
//!   is handed its hand's fire input in place of the game's own ([`substitute`], from the gun aim's
//!   per-weapon detour), so both hands fire the same way; the ammunition check's reload request
//!   and its cancel are refused for the local player
//!   ([`crate::controllers::weapons::reload`]), so one hand's gun never reloads or interrupts the
//!   other's; each armed gun's aim mode follows the wielded weapon's ([`input_tick`]); and the
//!   use camera hint a gun in a hand still needs is put back when another weapon of its hint stops
//!   being used ([`stopped_using`]).
//!
//! With the switch off, the wielded weapon fires from the game's own fire input, and an armed
//! weapon fires only while its magazine holds a round, since its ammunition check would otherwise
//! ask the character to reload, which reloads the wielded weapon. The dual-wield pair is left to
//! its own split ([`crate::controllers::weapons::dual_wield`]).
//!
//! The detours run on the simulation's worker threads; they read only the published inputs and
//! the armed weapons' addresses, and lock nothing but this module's short locks.

use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
    aim::aim::AimMode,
    camera::game_camera_manager::GameCameraManager,
    character::{
        character::Character,
        inventory::{Inventory, WeaponSlot},
    },
    equipment::weapon::{WeaponBase, WeaponInputs, WeaponState},
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    frame::Hand,
    loadout::live,
    weapons::{
        dual_wield::{self, single},
        hold::armed,
        reload,
    },
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_WEAPONS_PARALLEL_BINDER)
        .with_static_binder(&UPDATE_WEAPONS_SERIAL_BINDER)
        .with_static_binder(&STOP_USING_BASE_BINDER)
}

/// The per-hand tick's readout.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TickStatus {
    /// Whether the wielded weapon takes its hand's fire input in place of the game's.
    pub per_hand: bool,
    /// Each hand's fire input as of the last input tick, by [`Hand::index`].
    pub fire: [bool; 2],
    /// The updates the mod gave each hand's armed weapon since the payload loaded.
    pub updates: [u64; 2],
    /// The wielded weapon's updates handed its hand's fire input since the payload loaded.
    pub substituted: u64,
    /// The armed guns' aim modes set and the use hints put back since the payload loaded.
    pub aim_modes: u64,
    pub hints: u64,
}

pub fn status() -> TickStatus {
    TickStatus {
        per_hand: PER_HAND.load(Ordering::Relaxed),
        fire: Hand::BOTH.map(|h| FIRE[h.index()].load(Ordering::Relaxed)),
        updates: Hand::BOTH.map(|h| UPDATES[h.index()].load(Ordering::Relaxed)),
        substituted: SUBSTITUTED.load(Ordering::Relaxed),
        aim_modes: AIM_MODES.load(Ordering::Relaxed),
        hints: HINTS.load(Ordering::Relaxed),
    }
}

/// Whether the per-hand tick hands the wielded weapon its hand's input and refuses the ammunition
/// check's reload requests, as of the last input tick.
pub(crate) fn per_hand() -> bool {
    PER_HAND.load(Ordering::Relaxed)
}

/// Publish each hand's fire input for the tick's updates, by [`Hand::index`]: its trigger pulled
/// while nothing of the mod's holds its gun back.
pub(crate) fn publish_fire(fire: [bool; 2]) {
    for hand in Hand::BOTH {
        FIRE[hand.index()].store(fire[hand.index()], Ordering::Relaxed);
    }
}

/// Publish each hand's aim point, from its gun's sight line, at the aim update, by [`Hand::index`].
pub(crate) fn publish_targets(targets: [Option<Vec3>; 2]) {
    let now = Instant::now();
    *TARGETS.lock() = targets.map(|t| t.filter(|t| t.is_finite()).map(|t| (t, now)));
}

/// One input tick: publish `per_hand` (the setting), and with it on, hand each armed gun the
/// wielded weapon's aim mode (the player's, which the game forwards to the selected weapon alone)
/// and put back the use hints that a gun in a hand still needs.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn input_tick(owner: &mut Character, wielded: Option<usize>, per_hand: bool) {
    PER_HAND.store(per_hand, Ordering::Relaxed);
    let stopped = std::mem::take(&mut *STOPPED.lock());
    if !per_hand {
        return;
    }
    let mode = wielded
        .and_then(|w| unsafe { (w as *const WeaponBase).as_ref() })
        .map_or(AimMode::AIM_NORMAL, |w| w.m_AimMode);
    let in_hands: Vec<&mut WeaponBase> = Hand::BOTH
        .into_iter()
        .filter_map(|hand| unsafe { armed::weapon_of(owner, hand) })
        .collect();
    for weapon in in_hands {
        if weapon.m_AimMode != mode {
            unsafe { weapon.SetAimMode(mode) };
            AIM_MODES.fetch_add(1, Ordering::Relaxed);
        }
    }
    unsafe { restore_hints(owner, wielded, &stopped) };
}

/// Stop acting: the hands fire nothing.
pub(crate) fn stand_down() {
    publish_fire([false; 2]);
    *TARGETS.lock() = [None; 2];
}

/// The fire input for the local player's wielded `weapon`, updated by the inventory's fan-out with
/// the game's `input`, when the per-hand tick hands it its hand's own: the weapon is in a hand of
/// the table's, wielded, and not the dual-wield pair.
///
/// # Safety
///
/// `weapon` must be live, called from its per-tick update before the original runs.
pub(crate) unsafe fn substitute(weapon: &WeaponBase, input: &WeaponInputs) -> Option<WeaponInputs> {
    if !(PER_HAND.load(Ordering::Relaxed) && weapon.m_IsUserLocalPlayer) {
        return None;
    }
    let address = weapon as *const WeaponBase as usize;
    let hand = live::hand_of(address)?;
    let wielded = matches!(
        weapon.m_State,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    );
    let pair = weapon.m_EquipSlot == WeaponSlot::E_SLOT_DUAL_WIELD && !single::is(address);
    if !wielded || pair || dual_wield::attach::owns(address) {
        return None;
    }
    // SAFETY: the weapon's user is the live local player while it updates.
    let user = unsafe { (weapon.m_User[0] as *const Character).as_ref() }?;
    let fire = FIRE[hand.index()].load(Ordering::Relaxed)
        && !reload::holds_fire(address, hand)
        && unsafe { user.m_Inventory.GetAnimationAllowsShooting() };
    SUBSTITUTED.fetch_add(1, Ordering::Relaxed);
    Some(WeaponInputs {
        m_PrimaryFireInput: if fire { 1.0 } else { 0.0 },
        ..*input
    })
}

static PER_HAND: AtomicBool = AtomicBool::new(false);
static FIRE: [AtomicBool; 2] = [const { AtomicBool::new(false) }; 2];
static TARGETS: Mutex<[Option<(Vec3, Instant)>; 2]> = Mutex::new([None; 2]);
static UPDATES: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
static SUBSTITUTED: AtomicU64 = AtomicU64::new(0);
static AIM_MODES: AtomicU64 = AtomicU64::new(0);
static HINTS: AtomicU64 = AtomicU64::new(0);
/// The use hints of the local player's weapons that stopped being used since the last input tick.
static STOPPED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// How long the aim update's point stays current for an armed weapon's update.
const MAX_TARGET_AGE: Duration = Duration::from_millis(250);
/// More use hints than stop between two input ticks.
const MAX_STOPPED: usize = 16;

/// Put back each of the `stopped` use hints that a weapon in a hand (the wielded one, or one armed)
/// still in use by `owner` carries, when the hint table no longer has it.
unsafe fn restore_hints(owner: &mut Character, wielded: Option<usize>, stopped: &[u32]) {
    if stopped.is_empty() {
        return;
    }
    let Some(camera) = (unsafe { GameCameraManager::get() }) else {
        return;
    };
    let armed = Hand::BOTH
        .into_iter()
        .filter_map(|hand| unsafe { armed::weapon_of(owner, hand) })
        .map(|w| w as *const WeaponBase);
    let wielded = wielded.map(|w| w as *const WeaponBase);
    let needed: Vec<u32> = wielded
        .into_iter()
        .chain(armed)
        .filter_map(|w| unsafe { w.as_ref() })
        .filter(|w| std::ptr::eq(w.m_User[0].cast::<Character>(), owner))
        .map(|w| w.m_CameraHash)
        .collect();
    let mut restored = Vec::new();
    for &hint in stopped {
        if hint == 0 || !needed.contains(&hint) || restored.contains(&hint) {
            continue;
        }
        if !unsafe { camera.HasHint(hint) } {
            unsafe { camera.AddHint(hint, std::ptr::null()) };
            HINTS.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(target: "controllers", hint, "per-hand tick: put back a gun's camera hint");
        }
        restored.push(hint);
    }
}

/// The armed weapon's inputs and aim point for one update, or `None` without a current aim point.
unsafe fn update_arguments(
    inventory: &Inventory,
    weapon: &WeaponBase,
    hand: Hand,
) -> Option<(WeaponInputs, Vector3)> {
    let (target, at) = TARGETS.lock()[hand.index()]?;
    if at.elapsed() > MAX_TARGET_AGE {
        return None;
    }
    let address = weapon as *const WeaponBase as usize;
    // Without the per-hand tick an empty gun fires nothing: its ammunition check would ask the
    // character to reload, which reloads the wielded weapon.
    let loaded = PER_HAND.load(Ordering::Relaxed)
        || (weapon.m_MagazineAmmo > 0 && !weapon.m_WeaponIsWithoutMagazine);
    let fire = loaded
        && FIRE[hand.index()].load(Ordering::Relaxed)
        && !reload::holds_fire(address, hand)
        && unsafe { inventory.GetAnimationAllowsShooting() };
    Some((
        WeaponInputs {
            m_PrimaryFireInput: if fire { 1.0 } else { 0.0 },
            m_SecondaryFireInput: 0.0,
        },
        Vector3 {
            data: target.to_array(),
        },
    ))
}

/// Update each weapon armed in a hand of `inventory`'s, after its own fan-out, with `update`.
unsafe fn update_armed(
    inventory: *mut Inventory,
    update: impl Fn(&mut WeaponBase, &WeaponInputs, &Vector3),
) {
    for hand in Hand::BOTH {
        let Some(address) = armed::in_hand(hand) else {
            continue;
        };
        let Some(weapon) = (unsafe { armed::weapon_in(inventory, address) }) else {
            continue;
        };
        // SAFETY: `weapon_in` found the inventory live.
        let Some((inputs, aim)) = (unsafe { update_arguments(&*inventory, weapon, hand) }) else {
            continue;
        };
        update(weapon, &inputs, &aim);
        UPDATES[hand.index()].fetch_add(1, Ordering::Relaxed);
    }
}

#[detour(address = jc3gi::character::inventory::Inventory::UpdateWeapons_Parallel_ADDRESS)]
fn update_weapons_parallel(
    inventory: *mut Inventory,
    dt: f32,
    muzzle_target: *const Vector3,
    grenade_target: *const Vector3,
) {
    UPDATE_WEAPONS_PARALLEL
        .get()
        .unwrap()
        .call(inventory, dt, muzzle_target, grenade_target);
    unsafe {
        update_armed(inventory, |weapon, inputs, aim| {
            weapon.UpdateFromUserPostSim_Parallel(dt, aim, inputs)
        })
    };
}

#[detour(address = jc3gi::character::inventory::Inventory::UpdateWeapons_Serial_ADDRESS)]
fn update_weapons_serial(
    inventory: *mut Inventory,
    dt: f32,
    muzzle_target: *const Vector3,
    grenade_target: *const Vector3,
) {
    UPDATE_WEAPONS_SERIAL
        .get()
        .unwrap()
        .call(inventory, dt, muzzle_target, grenade_target);
    unsafe {
        update_armed(inventory, |weapon, inputs, aim| {
            weapon.UpdateFromUserPostSim_Serial(dt, aim, inputs)
        })
    };
}

/// The base of every weapon's stop-using, noting the use hint of a local player's weapon for the
/// next input tick ([`restore_hints`]): the hint table is keyed by hash, so the stop removes a hint
/// that the other hand's gun of the same hint still needs.
#[detour(address = jc3gi::equipment::weapon::WeaponBase::StopUsingBase_ADDRESS)]
fn stop_using_base(weapon: *mut WeaponBase) {
    let hint = unsafe { weapon.as_ref() }
        .filter(|w| w.m_IsUserLocalPlayer)
        .map(|w| w.m_CameraHash);
    STOP_USING_BASE.get().unwrap().call(weapon);
    if let Some(hint) = hint.filter(|&h| h != 0) {
        let mut stopped = STOPPED.lock();
        if stopped.len() < MAX_STOPPED && !stopped.contains(&hint) {
            stopped.push(hint);
        }
    }
}
