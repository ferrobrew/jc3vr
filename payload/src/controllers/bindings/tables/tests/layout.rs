//! The shipped default layout against the one the raw-input tables encoded: whatever a player
//! could reach before stays reachable, but for the actions the new layout drops on purpose.

use std::collections::BTreeSet;

use super::*;
use crate::{
    controllers::{
        bindings::tables::{
            coverage::{all_contexts, all_modes, injected, injected_with},
            tests::equivalence::legacy_layout,
        },
        vehicles::{
            config::WingsuitGesturesConfig,
            wingsuit_gestures::{
                FlightPose, GestureInput, GestureSample, HandClaims, WingsuitGestures,
            },
        },
    },
    vr::input::flight_pose::desired_for,
};

/// The game actions the new layout leaves without a button: grenades come from their pouch and
/// C4 from its chest pouch, and the grenade throw stays as an unbound action for a player who
/// binds it. The gamepad's B (releasing the tethers, cancelling, and stepping off a ledge) is the
/// grapple button's double tap ([`crate::controllers::bindings::grapple_button`]), a gesture the
/// tables do not declare; the use button held also steps off a ledge, and keeps a ragdoll one,
/// which needs a ragdoll the coverage does not simulate.
const DROPPED: [Action; 6] = [
    THROW_GRENADE,
    PLANT_EXPLOSIVE,
    RELEASE_GRAPPLE,
    CANCEL,
    BYPASS_FALL_PREVENTION,
    BTN_B,
];

// Suit gestures use the narrow native actions, not the broad button aliases.
const WINGSUIT_DROPPED_BUTTON_ALIASES: [Action; 3] = [ENTER_VEHICLE, USE_ITEM, BTN_Y];

#[test]
fn every_action_reachable_before_stays_reachable() {
    let mut added = BTreeSet::new();
    for mode in all_modes() {
        let gestures = gesture_actions(mode);
        for (dominant, ctx) in all_contexts() {
            let before = injected_with(mode, dominant, ctx, legacy_layout);
            let mut now = injected(mode, dominant, ctx);
            added.extend(now.difference(&before).copied());
            now.extend(gestures.iter().copied());
            let lost: Vec<_> = before
                .difference(&now)
                .filter(|a| !DROPPED.contains(a))
                .filter(|a| {
                    mode != Mode::Airborne(AirborneVariant::Wingsuit)
                        || !WINGSUIT_DROPPED_BUTTON_ALIASES.contains(a)
                })
                .collect();
            assert!(
                lost.is_empty(),
                "{mode:?}, {dominant:?}-handed, {ctx:?}: {lost:?} no longer reachable"
            );
        }
    }
    // The new layout adds the cutscene skip on the right B. The weapon slot select on a pad ships
    // unbound: the off-hand trackpad lies under the thumb's way to the debug panel chord.
    assert_eq!(added, BTreeSet::from([SKIP_CUTSCENE]));
}

fn gesture_actions(mode: Mode) -> BTreeSet<Action> {
    let expected = match mode {
        Mode::Airborne(AirborneVariant::Falling) => BTreeSet::from([OPEN_WINGSUIT]),
        Mode::Airborne(AirborneVariant::Wingsuit) => {
            BTreeSet::from([OPEN_WINGSUIT, CANCEL, WINGSUIT_AIRBRAKE])
        }
        _ => return BTreeSet::new(),
    };
    let config = WingsuitGesturesConfig::new();
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
                    gameplay: true,
                    canopy_requested: false,
                    pose: Some(pose),
                    claims: [HandClaims::default(); 2],
                },
                &config,
            );
            for desired in desired_for(commands, mode, false).unwrap() {
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
fn the_dominant_upper_button_enters_and_exits_vehicles() {
    for dominant in Hand::BOTH {
        let c = controls(dominant, |dom, _| dom.b = down());
        let step = Bindings::default().sim_step(ON_FOOT, &c, DT, &CONFIG);
        assert_eq!(player(&step, ENTER_VEHICLE), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, RELOAD), None);
        for class in [
            VehicleClass::Car,
            VehicleClass::Helicopter,
            VehicleClass::Plane,
        ] {
            let seat = Mode::Vehicle(VehicleSeat {
                class,
                driver: true,
            });
            let step = Bindings::default().sim_step(seat, &c, DT, &CONFIG);
            assert_eq!(
                player(&step, EXIT_VEHICLE),
                Some(Want::Hold(1.0)),
                "{class:?}"
            );
            assert_eq!(player(&step, USE_VEHICLE_MOD), None, "{class:?}");
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
        let step = Bindings::default().sim_step(ON_FOOT, &c, DT, &CONFIG);
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
