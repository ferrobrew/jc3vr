//! Registers the systems above the aim with the aim: the weapon handling's and the hand bodies'
//! hooks into the aim update and the per-weapon update ([`jc3vr_body::controllers::aim::hooks`]), the
//! weapon handling's contributions around the local player's weapon handling
//! ([`jc3vr_body::controllers::aim_state::PreWeaponHook`]), and the vehicle handling's mounted-gun fact
//! ([`jc3vr_body::controllers::facts::mounted`]).
//!
//! The aim calls into each of these on every update, so the install refuses to finish with one
//! missing: [`register`] panics naming it.

use std::fmt::Write as _;

use jc3gi::character::character::Character;
use jc3vr_body::controllers::{
    aim::{hooks, hooks::AimHooks},
    aim_state,
    aim_state::{PreWeaponHook, PreWeaponHookFn},
    facts::mounted,
};
use jc3vr_physical::controllers::physical_hands;

use crate::controllers::{
    vehicles,
    weapons::{
        c4, collide,
        dual_wield::single,
        grenade, hold,
        hold::{button_pickup, tick as hold_tick, use_item},
        unarmed_fire,
    },
};

pub(super) fn register() {
    hooks::register_aim_hooks(AimHooks {
        observe_grenade_hands: grenade::observe_hands,
        observe_c4_hands: c4::observe_hands,
        probe_c4_stick: c4::stick::probe,
        probe_collide: collide::probe,
        publish_armed_targets: hold_tick::publish_targets,
        grenade_fallback_target: grenade::fallback_target,
        after_aim: physical_hands::after_aim,
        pin_single: single::pin,
        before_update: hold::before_update,
        substitute_input: hold_tick::substitute,
    });
    for (hook, run) in PRE_WEAPON_HOOKS {
        aim_state::register_pre_weapon_hook(hook, run);
    }
    vehicles::register_mounted::register();
    let missing = missing();
    assert!(
        missing.is_empty(),
        "aim: the install left contributions unregistered:{missing}"
    );
}

/// The weapon handling's contributions around the local player's weapon handling, in the order
/// they run.
const PRE_WEAPON_HOOKS: [(PreWeaponHook, PreWeaponHookFn); 3] = [
    (PreWeaponHook::UnarmedFire, unarmed_fire_held_back),
    (PreWeaponHook::ButtonPickup, button_pickup_held_back),
    (PreWeaponHook::UseItem, use_item_before),
];

/// The fire and precision-aim inputs that would draw a weapon held back around the rest.
unsafe fn unarmed_fire_held_back(
    c: &mut Character,
    holstering: bool,
    rest: &mut dyn FnMut(&mut Character),
) {
    let held = unsafe { unarmed_fire::hold_back(c, holstering) };
    rest(c);
    if let Some(held) = held {
        unsafe { held.restore(c) };
    }
}

/// The use-item input that would pick a weapon up by button held back around the rest.
unsafe fn button_pickup_held_back(
    c: &mut Character,
    _holstering: bool,
    rest: &mut dyn FnMut(&mut Character),
) {
    let held = unsafe { button_pickup::hold_back(c) };
    rest(c);
    if let Some(held) = held {
        unsafe { held.restore(c) };
    }
}

/// A swimming pickup's slot emptied before the rest.
unsafe fn use_item_before(
    c: &mut Character,
    _holstering: bool,
    rest: &mut dyn FnMut(&mut Character),
) {
    unsafe { use_item::before_weapon_handling(c) };
    rest(c);
}

/// Every contribution the aim reads that nobody registered, one per line.
fn missing() -> String {
    let mut missing = String::new();
    if !hooks::is_registered() {
        missing.push_str("\n- the aim hooks");
    }
    for hook in aim_state::missing_pre_weapon_hooks() {
        let _ = write!(missing, "\n- pre-weapon hook {hook:?}");
    }
    if !mounted::is_registered() {
        missing.push_str("\n- the mounted-gun source");
    }
    missing
}
