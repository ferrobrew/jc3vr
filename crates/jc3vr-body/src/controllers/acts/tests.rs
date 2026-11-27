//! The act filters' decisions, one row per situation.

use test_case::test_case;

use crate::{
    controllers::{
        acts::{
            ActAction, ActClaim, ActDecision, Entry, Expiry, Guard, Owner, PRECEDENCE, Refusal,
            Role,
            claim::{first_claim, rank, settle},
        },
        aim_state::{ActVerdict, WatchedAct, local_verdict},
        locomotion::acts::{ActGates, ActScope, body_turn_passes},
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

const ACT: i32 = 7;
const STRAIGHT: i32 = 8;

fn claim(owner: Owner, action: ActAction) -> ActClaim {
    ActClaim {
        owner,
        act: ACT,
        action,
    }
}

/// The owners at `entry` that claim, as `claims` lists them; returns the winner and the owners
/// asked, in the order they were asked.
fn ask_at(entry: Entry, claims: &[(Owner, ActAction)]) -> (Option<Owner>, Vec<Owner>) {
    let mut asked = Vec::new();
    let winner = first_claim(entry, |owner| {
        asked.push(owner);
        claims
            .iter()
            .find(|(o, _)| *o == owner)
            .map(|(o, action)| claim(*o, *action))
    });
    (winner.map(|c| c.owner), asked)
}

const ALL_CHARACTER: [Owner; 3] = [Owner::VehicleReverse, Owner::MentalState, Owner::Locomotion];
const REPLACE: ActAction = ActAction::Replace {
    with: STRAIGHT,
    refused: Refusal::QueueOriginal,
};

#[test_case(Entry::Character, &[] => (None, ALL_CHARACTER.to_vec()); "no claim asks every character filter")]
#[test_case(Entry::Character, &[(Owner::VehicleReverse, ActAction::Drop), (Owner::Locomotion, ActAction::Drop)] => (Some(Owner::VehicleReverse), vec![Owner::VehicleReverse]); "the reverse drop is asked first and stops the rest")]
#[test_case(Entry::Character, &[(Owner::MentalState, ActAction::Drop), (Owner::Locomotion, REPLACE)] => (Some(Owner::MentalState), vec![Owner::VehicleReverse, Owner::MentalState]); "a swallowed holster never reaches the locomotion filter or its watchdog")]
#[test_case(Entry::Character, &[(Owner::Locomotion, REPLACE)] => (Some(Owner::Locomotion), ALL_CHARACTER.to_vec()); "the locomotion filter is asked last")]
#[test_case(Entry::Character, &[(Owner::WingsuitEvade, ActAction::Drop)] => (None, ALL_CHARACTER.to_vec()); "the barrel roll is not decided at the character entry")]
#[test_case(Entry::Model, &[(Owner::VehicleReverse, ActAction::Drop)] => (None, vec![Owner::WingsuitEvade]); "the model entry asks only the wingsuit")]
#[test_case(Entry::Model, &[(Owner::WingsuitEvade, ActAction::Drop)] => (Some(Owner::WingsuitEvade), vec![Owner::WingsuitEvade]); "the barrel roll is dropped at the model entry")]
fn precedence(entry: Entry, claims: &[(Owner, ActAction)]) -> (Option<Owner>, Vec<Owner>) {
    ask_at(entry, claims)
}

#[test_case(None, true => ActDecision::Pass; "no claim passes")]
#[test_case(Some(ActAction::Drop), true => ActDecision::Drop; "a drop drops")]
#[test_case(Some(REPLACE), true => ActDecision::Replace(STRAIGHT); "a taken replacement replaces")]
#[test_case(Some(REPLACE), false => ActDecision::Pass; "a refused start queues the original")]
#[test_case(Some(ActAction::Replace { with: STRAIGHT, refused: Refusal::Drop }), false => ActDecision::Drop; "a refused stunt turn is dropped")]
#[test_case(Some(ActAction::Queue(Guard::Unguarded)), true => ActDecision::Pass; "a queue claim never filters")]
fn settlement(action: Option<ActAction>, takes: bool) -> ActDecision {
    settle(action.map(|a| claim(Owner::Locomotion, a)), |with| {
        assert_eq!(with, STRAIGHT);
        takes
    })
}

#[test]
fn the_pre_flight_is_asked_only_for_a_replacement() {
    for action in [None, Some(ActAction::Drop)] {
        settle(action.map(|a| claim(Owner::Locomotion, a)), |_| {
            panic!("pre-flighted a non-replacement")
        });
    }
}

#[test]
fn every_owner_has_one_place_in_the_table() {
    for owner in [
        Owner::VehicleReverse,
        Owner::MentalState,
        Owner::Locomotion,
        Owner::WingsuitEvade,
        Owner::WeaponReconcile,
        Owner::SlideStart,
    ] {
        assert_eq!(
            PRECEDENCE.iter().filter(|(o, _)| *o == owner).count(),
            1,
            "{owner:?}"
        );
    }
}

#[test]
fn the_submitters_flush_after_the_filters_in_table_order() {
    let submitters: Vec<_> = PRECEDENCE
        .iter()
        .filter(|(_, role)| *role == Role::Submit)
        .map(|(owner, _)| *owner)
        .collect();
    assert_eq!(submitters, [Owner::WeaponReconcile, Owner::SlideStart]);
    assert!(rank(Owner::WeaponReconcile) < rank(Owner::SlideStart));
}

#[test_case(ActAction::Drop => Expiry::ThisQueue)]
#[test_case(REPLACE => Expiry::ThisQueue)]
#[test_case(ActAction::Queue(Guard::PreFlight) => Expiry::NextFlush)]
fn expiry(action: ActAction) -> Expiry {
    claim(Owner::SlideStart, action).expiry()
}
