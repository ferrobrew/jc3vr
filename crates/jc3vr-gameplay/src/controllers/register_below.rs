//! Registers the physical hands, the vehicle handling, and the weapon handling with the systems
//! below them: the body, the grip model, the roomscale, the hand bodies' step, the per-shot seam,
//! the weapon take, the weapon pickup hand-off, and the character hooks. Each subsystem's own
//! `register_body` names what it contributes.
//!
//! Each of these does real work every tick, and the body destruction observer keeps the climb and
//! the carried bodies from holding a destroyed body, so the install refuses to finish with one
//! missing: [`register`] panics naming it.

use std::fmt::Write as _;

use jc3vr_body::{
    controllers::{
        body::{mirror, rig::root},
        facts::climb,
        grab::tick as grab_tick,
        held::pickup,
        roomscale,
    },
    hooks::character::{
        bodies_destroying_registered, missing_auto_attach_blockers, serial_consumer_registered,
    },
};

use crate::controllers::{
    physical_hands,
    physical_hands::physics as hand_bodies,
    vehicles, weapons,
    weapons::{fire, hold::take, inventory},
};

pub(super) fn register() {
    physical_hands::register_body::register();
    vehicles::register_body::register();
    weapons::register_body::register();
    let missing = missing();
    assert!(
        missing.is_empty(),
        "controllers: the install left contributions unregistered:{missing}"
    );
}

/// Every contribution still unregistered, one per line, or empty.
fn missing() -> String {
    let mut missing = String::new();
    if !climb::is_registered() {
        missing.push_str("\n- the climb's readers");
    }
    if !root::palm_push_registered() {
        missing.push_str("\n- the palms' push");
    }
    for blocker in missing_auto_attach_blockers() {
        let _ = write!(missing, "\n- auto-attach blocker {blocker:?}");
    }
    if !bodies_destroying_registered() {
        missing.push_str("\n- the body destruction observer");
    }
    if !grab_tick::palm_summary_registered() {
        missing.push_str("\n- the palm summary");
    }
    if !roomscale::take_off_registered() {
        missing.push_str("\n- the wingsuit's take-off");
    }
    if !hand_bodies::guns::targets::vehicle_targets_registered() {
        missing.push_str("\n- the vehicle controls' targets for the hand bodies");
    }
    if !inventory::held_props_registered() {
        missing.push_str("\n- the held props reader");
    }
    if !hand_bodies::rail_step_registered() {
        missing.push_str("\n- the vehicle rail's pre-simulation step");
    }
    if !fire::mounted_shot_observer_registered() {
        missing.push_str("\n- the mounted shot observer");
    }
    if !take::refusal_sink_registered() {
        missing.push_str("\n- the take refusal sink");
    }
    if !pickup::sink_registered() {
        missing.push_str("\n- the weapon pickup sink");
    }
    if !serial_consumer_registered() {
        missing.push_str("\n- the magazine serial consumer");
    }
    if !mirror::attachment_sampler_registered() {
        missing.push_str("\n- the body mirror's attachment sampler");
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registration_leaves_nothing_missing() {
        static REGISTERED: std::sync::Once = std::sync::Once::new();
        REGISTERED.call_once(register);
        assert_eq!(missing(), "");
    }
}
