//! The act filters' decisions, one row per situation.

use test_case::test_case;

use crate::{
    controllers::{
        aim_state::{ActVerdict, WatchedAct, local_verdict},
        locomotion::acts::{ActDecision, ActGates, ActScope, body_turn_passes},
    },
    hooks::input::wingsuit::barrel_roll_dropped,
};

const OPEN: ActGates = ActGates {
    vr: true,
    in_gameplay: true,
    controllers: true,
};

#[test_case(None, true, true, false => ActVerdict::Pass; "an unwatched act passes")]
#[test_case(Some(WatchedAct::UnequipSlow), false, true, false => ActVerdict::Swallow; "the slow unequip is swallowed outside the weapon handling")]
#[test_case(Some(WatchedAct::UnequipSlow), true, true, true => ActVerdict::Swallow; "the slow unequip is swallowed inside it, deliberate or not")]
#[test_case(Some(WatchedAct::UnequipSlow), true, false, false => ActVerdict::Pass; "the slow unequip passes when the cuts do not act")]
#[test_case(Some(WatchedAct::UnequipSlow), false, false, false => ActVerdict::Pass; "nothing is swallowed outside the scope without the cuts")]
#[test_case(Some(WatchedAct::Holster), false, true, false => ActVerdict::Pass; "a holster from outside the weapon handling passes")]
#[test_case(Some(WatchedAct::Holster), true, true, false => ActVerdict::Swallow; "the relaxed branch's holster is swallowed")]
#[test_case(Some(WatchedAct::Holster), true, false, false => ActVerdict::Swallow; "a holster inside the scope needs no second gate")]
#[test_case(Some(WatchedAct::Holster), true, true, true => ActVerdict::PassDeliberate; "a deliberate holster passes")]
fn mental_state_cuts(
    act: Option<WatchedAct>,
    in_weapon_handling: bool,
    controllers: bool,
    deliberate: bool,
) -> ActVerdict {
    local_verdict(act, in_weapon_handling, controllers, deliberate)
}

#[test_case(OPEN, true, true => true; "the stick drives the body on the ground")]
#[test_case(OPEN, false, true => false; "the stick locomotion is off")]
#[test_case(OPEN, true, false => false; "off the ground or traversing")]
#[test_case(ActGates { vr: false, ..OPEN }, true, true => false; "the head pose is not the headset's")]
#[test_case(ActGates { in_gameplay: false, ..OPEN }, true, true => false; "gameplay does not own the character")]
#[test_case(ActGates { controllers: false, ..OPEN }, true, true => false; "the controllers are off")]
fn the_on_foot_scope_applies(gates: ActGates, stick: bool, on_ground: bool) -> bool {
    gates.applies_on_foot(stick, on_ground)
}

#[test_case(OPEN => true; "the stunt area needs only the gates")]
#[test_case(ActGates { vr: false, ..OPEN } => false; "not in the headset")]
#[test_case(ActGates { in_gameplay: false, ..OPEN } => false; "not in gameplay")]
#[test_case(ActGates { controllers: false, ..OPEN } => false; "controllers off")]
fn the_stunt_area_gates(gates: ActGates) -> bool {
    gates.open()
}

#[test_case(ActScope::OnFoot, ActDecision::Drop, true, true, false => true; "a standing turn of the rig plays the game's turn in place")]
#[test_case(ActScope::OnFoot, ActDecision::Drop, true, true, true => false; "the planted IK body keeps it dropped")]
#[test_case(ActScope::OnFoot, ActDecision::Drop, false, true, false => false; "the rig is not turning")]
#[test_case(ActScope::OnFoot, ActDecision::Drop, true, false, false => false; "the body is moving")]
#[test_case(ActScope::OnFoot, ActDecision::Replace(1), true, true, false => false; "a straightened start is never let through")]
#[test_case(ActScope::StuntArea, ActDecision::Drop, true, true, false => false; "the stunt area never lets a turn through")]
fn the_body_turn_passes(
    scope: ActScope,
    wanted: ActDecision,
    rig_turning: bool,
    standing_still: bool,
    ik_owns_idle: bool,
) -> bool {
    body_turn_passes(scope, wanted, rig_turning, standing_still, ik_owns_idle)
}

#[test_case(true, true, false => true; "the evade is off in the headset")]
#[test_case(true, true, true => false; "the evade is on")]
#[test_case(true, false, false => false; "the head pose is not the headset's")]
#[test_case(false, true, false => false; "the head pose is not live")]
fn the_barrel_roll_is_dropped(headpose_active: bool, vr: bool, evade_enabled: bool) -> bool {
    barrel_roll_dropped(headpose_active, vr, evade_enabled)
}
