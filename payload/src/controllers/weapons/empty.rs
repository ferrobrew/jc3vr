//! The empty weapon: the game's automatic switch away from it suppressed, and its dry fire felt in
//! the hand. The engine side is in `docs/engine/gameplay/weapon-selection.md`, "The switch away
//! from an empty weapon".
//!
//! When the player pulls the trigger of a weapon whose magazine is empty and which cannot reload,
//! the per-gun ammunition check ([`WeaponBase::CheckIfReloadShouldBeTriggeredBase`]) calls
//! [`Inventory::TryToSwitchToValidWeapon`], which switches to another weapon with ammunition.
//! Under the controller scheme the weapons are drawn and holstered by hand, so a gun that leaves
//! the hand on its own breaks that. Two detours scope the suppression to that one caller:
//!
//! - a detour on the ammunition check marks the check of the local player's weapon on its thread
//!   ([`CHECK`]) for the length of the call;
//! - a detour on the switch, inside a marked check and with `weapons.auto_switch_when_empty` off,
//!   reports that no weapon qualified without switching. The game then treats the pull as it does
//!   with no other weapon to switch to: the HUD's out-of-ammunition notice and the gun's
//!   `OutOfAmmoFire` effect at the pull's press edge, and no shot. The switch's other caller, the
//!   `NWeaponEquipTask` state task, runs outside a check and is left alone, as are manual
//!   selection, scripted switches, and the removal of a weapon, which never call it.
//!
//! The empty gun stays wielded, so it can be holstered at its holster as usual, and the reload
//! input does nothing until ammunition is picked up, since the game queues a reload only when the
//! weapon can reload. Both dual-wielded pistols share one magazine, so they run dry together and
//! both stay in the hands.
//!
//! A pull at the press edge that the check found empty plays [`Event::DryFire`] on the hand whose
//! gun was pulled, since the gun gives no recoil to feel: one that tried the switch, whether or
//! not the switch is suppressed, and one of an empty magazine with ammunition left,
//! whose reload the true reload stops ([`crate::controllers::weapons::reload`]). A gun the true
//! reload holds from firing (its chamber empty or its magazine out) has its shot cleared here as
//! well; its trigger already clicks at the input tick.
//!
//! With the per-hand tick ([`crate::controllers::weapons::hold::tick`]), every gun in a hand runs
//! its own check, and each check asks the character to reload the wielded weapon whenever its own
//! magazine runs low, and cancels the character's reload every tick for a gun that does not reload
//! by magazine. Inside a marked check both requests are refused for the local player
//! ([`refuses_check_request`], from the detours on [`Character::StartReloadingWieldingWeapon`] and
//! [`Character::CancelReload`]), so one hand's gun never reloads or interrupts the other's; the
//! true reload reloads each gun by hand.
//!
//! [`Character::StartReloadingWieldingWeapon`]: jc3gi::character::character::Character::StartReloadingWieldingWeapon

use std::{
    cell::Cell,
    sync::atomic::{AtomicU64, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    character::{character::Character, inventory::Inventory},
    equipment::weapon::{WeaponBase, WeaponData},
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        hands::ik,
        haptics::{self, Event},
        scheme,
        weapons::{fire, hold::tick, reload},
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CHECK_IF_RELOAD_SHOULD_BE_TRIGGERED_BASE_BINDER)
        .with_static_binder(&TRY_TO_SWITCH_TO_VALID_WEAPON_BINDER)
        .with_static_binder(&CANCEL_RELOAD_BINDER)
}

/// The empty-weapon readout.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct EmptyStatus {
    /// Automatic switches the game tried and the mod suppressed.
    pub suppressed: u64,
    /// Automatic switches the game tried and the mod let through.
    pub allowed: u64,
    /// Pulls of an empty gun at their press edge.
    pub dry_fires: u64,
    /// The ammunition checks' reload requests and cancels refused for the local player.
    pub requests_refused: u64,
}

pub fn status() -> EmptyStatus {
    EmptyStatus {
        suppressed: SUPPRESSED.load(Ordering::Relaxed),
        allowed: ALLOWED.load(Ordering::Relaxed),
        dry_fires: DRY_FIRES.load(Ordering::Relaxed),
        requests_refused: REQUESTS_REFUSED.load(Ordering::Relaxed),
    }
}

/// Whether to refuse a reload start or a cancel of `character`'s reload made now: it is the local
/// player's, inside a marked ammunition check on this thread, and the per-hand tick acts. Counts a
/// refusal.
pub(crate) fn refuses_check_request(character: *const Character) -> bool {
    let local = ik::local_character();
    let refuse =
        CHECK.get().is_some() && tick::per_hand() && local != 0 && character as usize == local;
    if refuse {
        REQUESTS_REFUSED.fetch_add(1, Ordering::Relaxed);
    }
    refuse
}

/// What the switch detour does with a call.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SwitchDecision {
    /// Let the game switch: the call is not the empty-weapon switch of the player's weapon, or the
    /// setting allows it.
    Pass,
    /// Report that no weapon qualified, without switching.
    Suppress,
}

/// Decide a call of the switch: `in_check` is whether it comes from a marked ammunition check of
/// the local player's weapon on this thread, and `auto_switch` the setting.
pub fn decide(in_check: bool, auto_switch: bool) -> SwitchDecision {
    if in_check && !auto_switch {
        SwitchDecision::Suppress
    } else {
        SwitchDecision::Pass
    }
}

/// Whether a marked check was a dry fire to pulse: the check tried the switch, which it does only
/// for a requested shot of an empty weapon that cannot reload, at the pull's press edge.
pub fn is_dry_fire(tried_switch: bool, was_trigger_pulled: bool) -> bool {
    tried_switch && !was_trigger_pulled
}

/// Whether a marked check was a dry fire of an empty gun that can still reload: a requested shot
/// of an empty magazine at the pull's press edge, which the check cleared. The check asks for a
/// reload rather than a switch, which the true reload stops
/// ([`crate::controllers::weapons::reload`]), so the pull only clicks.
pub fn is_reloadable_dry_fire(
    requested: bool,
    empty: bool,
    cleared: bool,
    was_trigger_pulled: bool,
) -> bool {
    requested && empty && cleared && !was_trigger_pulled
}

static SUPPRESSED: AtomicU64 = AtomicU64::new(0);
static ALLOWED: AtomicU64 = AtomicU64::new(0);
static DRY_FIRES: AtomicU64 = AtomicU64::new(0);
static REQUESTS_REFUSED: AtomicU64 = AtomicU64::new(0);

/// A marked ammunition check in progress on this thread.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Check {
    /// Whether the check called the switch.
    tried_switch: bool,
}

thread_local! {
    static CHECK: Cell<Option<Check>> = const { Cell::new(None) };
}

/// The check runs on the thread that updates the weapon, a worker thread in the parallel pass, so
/// the mark is per thread and nothing here touches the VR state.
#[detour(address = jc3gi::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggeredBase_ADDRESS)]
fn check_if_reload_should_be_triggered_base(
    weapon: *mut WeaponBase,
    gun_data: *mut WeaponData,
    should_try_to_fire: *mut bool,
    was_trigger_pulled: bool,
) {
    let original = || {
        CHECK_IF_RELOAD_SHOULD_BE_TRIGGERED_BASE
            .get()
            .unwrap()
            .call(weapon, gun_data, should_try_to_fire, was_trigger_pulled)
    };
    let acting = Config::lock_query(|c| c.controllers.enabled) && scheme::is_controllers();
    let local = unsafe { weapon.as_ref() }.is_some_and(|w| w.m_IsUserLocalPlayer);
    if !(acting && local) {
        return original();
    }
    let requested = unsafe { should_try_to_fire.as_ref() }.is_some_and(|s| *s);
    let outer = CHECK.replace(Some(Check {
        tried_switch: false,
    }));
    original();
    let check = CHECK.replace(outer);

    // SAFETY: the weapon is live for the length of its update, on this thread, and `gun_data` is
    // one of its guns.
    let hand = unsafe { weapon.as_ref() }.and_then(|w| unsafe { fire::gun_hand(w, gun_data) });
    // The reload holds the gun: its trigger is kept from the game and clicks at the input tick, so
    // the shot is only cleared here, against a buffered one.
    if hand.is_some_and(|hand| reload::holds_fire(weapon as usize, hand)) {
        if let Some(should) = unsafe { should_try_to_fire.as_mut() } {
            *should = false;
        }
        return;
    }
    let empty = unsafe { weapon.as_ref() }.is_some_and(|w| w.m_MagazineAmmo <= 0);
    let cleared = unsafe { should_try_to_fire.as_ref() }.is_some_and(|s| !*s);
    let dry = check.is_some_and(|c| {
        is_dry_fire(c.tried_switch, was_trigger_pulled)
            || is_reloadable_dry_fire(requested, empty, cleared, was_trigger_pulled)
    });
    if !dry {
        return;
    }
    DRY_FIRES.fetch_add(1, Ordering::Relaxed);
    if let Some(hand) = hand {
        haptics::play(hand, Event::DryFire);
    }
}

/// The character's reload cancel, refused inside the local player's marked ammunition check.
#[detour(address = jc3gi::character::character::Character::CancelReload_ADDRESS)]
fn cancel_reload(character: *mut Character) {
    if refuses_check_request(character) {
        return;
    }
    CANCEL_RELOAD.get().unwrap().call(character);
}

#[detour(address = jc3gi::character::inventory::Inventory::TryToSwitchToValidWeapon_ADDRESS)]
fn try_to_switch_to_valid_weapon(inventory: *mut Inventory) -> bool {
    let check = CHECK.get();
    if check.is_some() {
        CHECK.set(Some(Check { tried_switch: true }));
    }
    let auto_switch = Config::lock_query(|c| c.controllers.weapons.auto_switch_when_empty);
    match decide(check.is_some(), auto_switch) {
        SwitchDecision::Suppress => {
            SUPPRESSED.fetch_add(1, Ordering::Relaxed);
            false
        }
        SwitchDecision::Pass => {
            if check.is_some() {
                ALLOWED.fetch_add(1, Ordering::Relaxed);
            }
            TRY_TO_SWITCH_TO_VALID_WEAPON.get().unwrap().call(inventory)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_checks_switch_is_suppressed_and_only_when_the_setting_is_off() {
        assert_eq!(decide(true, false), SwitchDecision::Suppress);
        assert_eq!(decide(true, true), SwitchDecision::Pass);
        assert_eq!(decide(false, false), SwitchDecision::Pass);
        assert_eq!(decide(false, true), SwitchDecision::Pass);
    }

    #[test]
    fn an_empty_magazine_with_reserve_clicks_once_per_pull() {
        assert!(is_reloadable_dry_fire(true, true, true, false));
        assert!(!is_reloadable_dry_fire(true, true, true, true));
        assert!(!is_reloadable_dry_fire(false, true, true, false));
        assert!(!is_reloadable_dry_fire(true, false, true, false));
        assert!(!is_reloadable_dry_fire(true, true, false, false));
    }

    #[test]
    fn a_dry_fire_pulses_once_per_pull() {
        assert!(is_dry_fire(true, false));
        assert!(!is_dry_fire(true, true));
        assert!(!is_dry_fire(false, false));
        assert!(!is_dry_fire(false, true));
    }
}
