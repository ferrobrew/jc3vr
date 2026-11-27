//! Registers the systems above the aim with the aim: the weapon handling's and the hand bodies'
//! hooks into the aim update and the per-weapon update ([`jc3vr_body::controllers::aim::hooks`]),
//! and the weapon handling's contributions around the local player's weapon handling in their
//! order ([`PRE_WEAPON_HOOKS`]). The vehicle handling's mounted-gun fact
//! ([`jc3vr_body::controllers::facts::mounted`]) rides with its other body fields
//! ([`jc3vr_vehicles::register_body`]), which the register file for the systems below fills.
//!
//! Each fills a field of the body's contributions, which reports any still empty to the install's
//! registration check ([`jc3vr_body::contributions::register`]).

use jc3gi::character::character::Character;
use jc3vr_body::{
    contributions::BodyContributions,
    controllers::{aim::hooks::AimHooks, aim_state::PreWeaponHookFn},
};
use jc3vr_physical::controllers::physical_hands;
use jc3vr_weapons::controllers::weapons::{
    c4, collide,
    dual_wield::single,
    grenade, hold,
    hold::{button_pickup, tick as hold_tick, use_item},
    unarmed_fire,
};

pub(super) fn register(body: &mut BodyContributions) {
    body.aim_hooks = Some(AimHooks {
        observe_grenade_hands: grenade::observe_hands,
        observe_c4_hands: c4::observe_hands,
        probe_c4_stick: c4::stick::probe,
        probe_collide: collide::probe,
        publish_armed_targets: hold_tick::publish_targets,
        after_aim: physical_hands::after_aim,
        pin_single: single::pin,
        before_update: hold::before_update,
        substitute_input: hold_tick::substitute,
    });
    body.pre_weapon_hooks = &PRE_WEAPON_HOOKS;
}

/// The weapon handling's contributions around the local player's weapon handling, in the order
/// they run, each around the ones after it and the call: the fire and precision-aim inputs that
/// would draw a weapon with nothing in the hands are held back, then the use-item input that would
/// pick a weapon up by button, then a swimming pickup's slot is emptied before the call drops the
/// slot's weapon.
static PRE_WEAPON_HOOKS: [PreWeaponHookFn; 3] = [
    unarmed_fire_held_back,
    button_pickup_held_back,
    use_item_before,
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
