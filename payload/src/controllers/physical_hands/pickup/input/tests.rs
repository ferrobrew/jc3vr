use std::{num::NonZeroUsize, time::Instant};

use glam::{Quat, Vec3};
use jc3gi::vehicle::vehicle::DoorSlot;

use super::*;
use crate::controllers::{
    bindings::controls::ButtonControl,
    frame::Pose,
    grab::{PickupKind, latch::GripLatch},
    physical_hands::{
        characters::{CharacterTarget, CharactersConfig, Regrip, Seize},
        doors::{DoorTarget, Latch, config::TEARING, tearing::TearGrip},
        grasp::Grip,
        pickup::{
            PICKUP,
            diagnostics::InputState,
            state::{Held, HeldKind},
            tick::{self, JointInputs, TickInputs},
        },
        rigid::BodyPose,
    },
};

#[test]
fn open_grip_produces_and_consumes_release_for_every_owner_on_either_hand() {
    for hand in Hand::BOTH {
        for owner in [
            Owner::Held,
            Owner::Latch,
            Owner::Seize,
            Owner::Regrip,
            Owner::Door,
        ] {
            let mut state = PickupState::new();
            let i = hand.index();
            match owner {
                Owner::Held => state.held[i] = Some(held()),
                Owner::Latch => state.latches[i] = Some(latch()),
                Owner::Seize => state.seizes[i] = Some(seize()),
                Owner::Regrip => {
                    state.regrips[i] = Some(Regrip {
                        seize: seize(),
                        remaining_s: 0.0,
                    });
                }
                Owner::Door => state.doors[i] = Some(TearGrip::fixture(door(), 3)),
            }
            assert!(state.has_interaction(hand));
            let mut desired = Vec::new();
            produce(&mut state, false, [None; 2], &mut desired);
            assert_eq!(state.requests[i], Some(Request::Release), "owner {owner:?}");
            consume_without_native(&mut state);
            assert!(!state.has_interaction(hand), "owner {owner:?}");
            assert_eq!(state.requests, [None; 2]);
            assert!(desired.is_empty());
        }
    }
}

#[test]
fn open_grip_cancels_the_producers_pending_grab_before_acquisition() {
    for hand in Hand::BOTH {
        let mut state = PickupState::new();
        let mut reaches = [None; 2];
        reaches[hand.index()] = Some(reach());
        let mut desired = Vec::new();
        produce(&mut state, true, reaches, &mut desired);
        assert!(matches!(
            state.requests[hand.index()],
            Some(Request::ObservedGrab(_))
        ));
        assert!(state.has_interaction(hand));
        produce(&mut state, false, [None; 2], &mut desired);
        assert_eq!(state.requests[hand.index()], Some(Request::Release));
        consume_without_native(&mut state);
        assert!(!state.has_interaction(hand));
        assert!(desired.is_empty());
    }
}

#[test]
fn opening_one_hand_does_not_cancel_the_other_hands_interaction() {
    for hand in Hand::BOTH {
        let mut state = PickupState::new();
        state.seizes = [Some(seize()); 2];
        let mut held = [true; 2];
        held[hand.index()] = false;
        produce_hands(&mut state, held, [None; 2], &mut Vec::new());
        assert_eq!(state.requests[hand.index()], Some(Request::Release));
        assert_eq!(state.requests[hand.other().index()], None);
        assert_eq!(state.seizes[hand.other().index()], Some(seize()));
    }
}

#[test]
fn open_grip_also_replaces_a_pending_remote_grab() {
    for hand in Hand::BOTH {
        let mut state = PickupState::new();
        state.requests[hand.index()] = Some(Request::Grab(reach()));
        produce(&mut state, false, [None; 2], &mut Vec::new());
        assert_eq!(state.requests[hand.index()], Some(Request::Release));
    }
}

#[test]
fn cancel_all_discards_derived_entry_clicks_before_the_actual_input_producer() {
    for held in [false, true] {
        let mut state = PickupState::new();
        state.seizes = [Some(seize()); 2];
        state.hijack_clicks = [true; 2];
        state.drop_all();
        let mut desired = Vec::new();
        produce(&mut state, held, [None; 2], &mut desired);
        assert!(desired.is_empty());
        assert_eq!(state.hijack_clicks, [false; 2]);
        assert_eq!(state.requests, [None; 2]);
    }
}

#[test]
fn stale_input_cancellation_discards_a_queued_entry_click() {
    let mut state = PickupState::new();
    state.hijack_clicks = [true; 2];
    // SAFETY: stale input clears all owners before any native access.
    unsafe { tick::tick(&mut state, tick_inputs(), &PICKUP) };
    let mut desired = Vec::new();
    produce(&mut state, false, [None; 2], &mut desired);
    assert!(desired.is_empty());
}

#[test]
fn uncancelled_entry_click_is_emitted_once_and_is_not_a_grip_owner() {
    let mut state = PickupState::new();
    state.hijack_clicks[0] = true;
    assert!(!state.has_interaction(Hand::Left));
    let mut desired = Vec::new();
    produce(&mut state, false, [None; 2], &mut desired);
    assert_eq!(desired.len(), 1);
    assert_eq!(desired[0].target.action, Action::ENTER_VEHICLE);
    assert_eq!(desired[0].want, Want::Click);
    desired.clear();
    produce(&mut state, false, [None; 2], &mut desired);
    assert!(desired.is_empty());
    assert_eq!(state.requests, [None; 2]);
}

#[derive(Debug, Copy, Clone)]
enum Owner {
    Held,
    Latch,
    Seize,
    Regrip,
    Door,
}

fn produce(
    state: &mut PickupState,
    held: bool,
    reaches: [Option<Reach>; 2],
    desired: &mut Vec<Desired>,
) {
    produce_hands(state, [held; 2], reaches, desired);
}

fn produce_hands(
    state: &mut PickupState,
    held: [bool; 2],
    reaches: [Option<Reach>; 2],
    desired: &mut Vec<Desired>,
) {
    let grips = held.map(|held| ButtonControl {
        down: held,
        pressed: held,
        released: !held,
    });
    let grabs = held.map(|held| held.then_some(GrabVolume::Pickup(PickupKind::Prop)));
    let inputs = FinishInputs {
        grips,
        grabs,
        triggers: [ButtonControl::default(); 2],
        unavailable: [false; 2],
        reload_button: ButtonControl::default(),
        dt: 0.01,
    };
    let tick = crate::controllers::grab::tick::resolve(Vec::new(), Default::default(), None);
    let context = diagnostics::input(
        1,
        &tick,
        InputState {
            grips,
            grabs,
            latches: [GripLatch::Open; 2],
        },
        [None; 2],
    );
    assert!(finish(state, &inputs, reaches, true, desired, &context).is_empty());
}

fn consume_without_native(state: &mut PickupState) {
    state.input_at = Some(Instant::now());
    // SAFETY: generated releases remove all owners; no palms means no throws or shoves.
    unsafe { tick::tick(state, tick_inputs(), &PICKUP) };
}

fn tick_inputs() -> TickInputs {
    TickInputs {
        palms: [None; 2],
        space: None,
        joints: JointInputs::default(),
        dt: 0.01,
        time_scale: 1.0,
        unlatch_pull_m: 0.05,
        tearing: TEARING,
        world_scale: 1.0,
        characters: CharactersConfig::default(),
        reaches: [None; 2],
    }
}

fn reach() -> Reach {
    Reach {
        object: NonZeroUsize::new(8).unwrap(),
        body: 3,
        point: Vec3::ZERO,
        distance: 0.05,
        normal: Vec3::Y,
        kind: TargetKind::Prop { mass_kg: 5.0 },
    }
}

fn held() -> Held {
    Held {
        object: reach().object,
        body: 3,
        grip: Grip::of(
            Pose::default(),
            BodyPose {
                centre: Vec3::ZERO,
                orientation: Quat::IDENTITY,
            },
            Vec3::ZERO,
        ),
        mass_kg: 5.0,
        joined: true,
        kind: HeldKind::Articulated,
    }
}

fn door() -> DoorTarget {
    DoorTarget {
        vehicle: reach().object,
        vehicle_body: Some(3),
        part_index: 1,
        slot: DoorSlot::FRONT_LEFT,
        latched: true,
        outward: Vec3::X,
    }
}

fn latch() -> Latch {
    Latch {
        door: door(),
        vehicle_body: 3,
        grab: Vec3::ZERO,
        outward: Vec3::X,
        palm: Pose::default(),
        opening_at: None,
    }
}

fn seize() -> Seize {
    Seize {
        target: CharacterTarget {
            character: reach().object,
            body: 3,
            seated: None,
            ragdoll: false,
            hostile: true,
        },
        grab: Vec3::ZERO,
    }
}
