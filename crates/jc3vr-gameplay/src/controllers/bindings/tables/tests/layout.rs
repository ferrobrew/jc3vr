//! Properties of the shipped default layout: the wingsuit gestures' native actions, the entry and
//! exit buttons, and the unbound weapon slot select.

use std::collections::BTreeSet;

use jc3vr_vehicles::vehicles::{
    tuning::VEHICLES,
    wingsuit::gestures::{
        FlightPose, GestureInput, HandClaims, Stage, WingsuitGestures, desired_for,
    },
};
use jc3vr_view::vr::input::flight_pose::GestureSample;

use super::*;

#[test]
fn the_wingsuit_gestures_drive_their_native_actions() {
    for mode in [
        Mode::Airborne(AirborneVariant::Falling),
        Mode::Airborne(AirborneVariant::Wingsuit),
    ] {
        gesture_actions(mode);
    }
}

fn gesture_actions(mode: Mode) -> BTreeSet<Action> {
    let expected = match mode {
        Mode::Airborne(AirborneVariant::Falling) => BTreeSet::from([OPEN_WINGSUIT]),
        Mode::Airborne(AirborneVariant::Wingsuit) => {
            BTreeSet::from([OPEN_WINGSUIT, CANCEL, WINGSUIT_AIRBRAKE])
        }
        _ => return BTreeSet::new(),
    };
    let config = VEHICLES.get().wingsuit_gestures;
    let neutral = FlightPose {
        lateral: [0.0; 2],
        reach: [1.0 / 3.0; 2],
        height: [-1.0 / 3.0; 2],
        angle_deg: [90.0; 2],
        palm_roll_deg: [0.0; 2],
    };
    let flight = FlightPose {
        lateral: [0.30; 2],
        reach: [0.30; 2],
        height: [0.0; 2],
        angle_deg: [0.0; 2],
        palm_roll_deg: [0.0; 2],
    };
    let brake = FlightPose {
        palm_roll_deg: [90.0; 2],
        ..flight
    };
    let mut state = WingsuitGestures::default();
    let mut sample = GestureSample {
        epoch: 1,
        sequence: 0,
        time_ns: 0,
    };
    let mut actions = BTreeSet::new();
    for (pose, count) in [
        (neutral, 5),
        (flight, 6),
        (brake, 6),
        (flight, 3),
        (neutral, 9),
    ] {
        for _ in 0..count {
            sample.sequence += 1;
            sample.time_ns += 50_000_000;
            let commands = state.step(
                GestureInput {
                    sample,
                    mode,
                    airborne: false,
                    deploy_grapple: false,
                    gameplay: true,
                    canopy_requested: false,
                    pose: Some(pose),
                    claims: [HandClaims::default(); 2],
                },
                &config,
            );
            for desired in desired_for(commands, Stage::of(mode, false), false).unwrap() {
                assert_eq!(desired.target.map, MapKind::Player, "{mode:?}: {desired:?}");
                assert_eq!(
                    desired.want,
                    if desired.target.action == OPEN_WINGSUIT {
                        Want::Click
                    } else {
                        Want::Hold(1.0)
                    },
                    "{mode:?}: {desired:?}"
                );
                actions.insert(desired.target.action);
            }
        }
    }
    assert_eq!(actions, expected, "{mode:?}: native gesture actions");
    actions
}

#[test]
fn the_dominant_upper_button_enters_and_the_right_a_held_exits() {
    for dominant in Hand::BOTH {
        let c = controls(dominant, |dom, _| dom.b = down());
        let step = Bindings::default().sim_step(ON_FOOT, &c, DT);
        assert_eq!(player(&step, ENTER_VEHICLE), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, RELOAD), None);
        let mut a = Controls {
            dominant,
            ..Default::default()
        };
        a.hands[Hand::Right.index()].a = down();
        for class in [
            VehicleClass::Car,
            VehicleClass::Helicopter,
            VehicleClass::Plane,
        ] {
            let seat = Mode::Vehicle(VehicleSeat {
                class,
                driver: true,
            });
            // The dominant upper button fires in a vehicle; it neither exits nor burns nitrous.
            let step = Bindings::default().sim_step(seat, &c, DT);
            assert_eq!(player(&step, EXIT_VEHICLE), None, "{class:?}");
            assert_eq!(player(&step, USE_VEHICLE_MOD), None, "{class:?}");
            assert_eq!(
                player(&step, FIRE_VEHICLE_WEAPON_PRIMARY),
                Some(Want::Hold(1.0)),
                "{class:?}"
            );
            let mut b = Bindings::default();
            let mut exited = false;
            for _ in 0..30 {
                exited |= player(&b.sim_step(seat, &a, DT), EXIT_VEHICLE) == Some(Want::Hold(1.0));
            }
            assert!(exited, "{class:?}");
        }
    }
}

#[test]
fn the_off_hand_trackpad_selects_no_weapon_slot_by_default() {
    // The slot select on a pad ships unbound: the off-hand trackpad lies under the thumb's way to
    // the debug panel chord's stick click, so a press in any quadrant selects nothing.
    for pad in [
        Vec2::new(0.0, 0.8),
        Vec2::new(0.8, 0.1),
        Vec2::new(-0.8, 0.0),
        Vec2::new(0.1, -0.8),
        Vec2::new(0.05, 0.1),
    ] {
        let c = controls(Hand::Right, |_, off| {
            off.trackpad_left = down();
            off.trackpad = pad;
        });
        let step = Bindings::default().sim_step(ON_FOOT, &c, DT);
        let selected: Vec<_> = step
            .desired
            .iter()
            .map(|d| d.target.action)
            .filter(|a| {
                matches!(
                    a,
                    SELECT_DUEL_WIELD
                        | SELECT_TWO_HANDED
                        | SELECT_TWO_HANDED_SPECIAL
                        | SELECT_EXPLOSIVES
                )
            })
            .collect();
        assert!(selected.is_empty(), "{pad:?}: {selected:?}");
    }
}
