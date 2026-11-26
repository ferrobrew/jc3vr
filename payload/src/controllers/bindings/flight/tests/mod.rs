mod current;

use jc3gi::input::input_action_map::Action::{CANCEL, OPEN_WINGSUIT, WINGSUIT_AIRBRAKE};

use super::*;
use crate::controllers::{
    bindings::hold::{Target, Want},
    frame::ActionSetKind,
    loadout::Socket,
    mode::AirborneVariant,
    vehicles::{
        config::WingsuitGesturesConfig,
        wingsuit_gestures::{FlightPose, GestureSample, SuitTransition},
    },
};

const SUIT: Mode = Mode::Airborne(AirborneVariant::Wingsuit);
const CONFIG: WingsuitGesturesConfig = WingsuitGesturesConfig {
    neutral_dwell_s: 0.125,
    flight_dwell_s: 0.25,
    close_dwell_s: 0.25,
    brake_dwell_s: 0.125,
    max_sample_gap_s: 0.5,
    ..WingsuitGesturesConfig::new()
};
const FLIGHT: FlightPose = FlightPose {
    lateral: [0.30; 2],
    reach: [0.32; 2],
    height: [0.0; 2],
    angle_deg: [5.0; 2],
    palm_roll_deg: [0.0; 2],
};
const LOWERED: FlightPose = FlightPose {
    lateral: [0.03; 2],
    reach: [0.33; 2],
    height: [-0.30; 2],
    angle_deg: [90.0; 2],
    palm_roll_deg: [0.0; 2],
};
const BRAKE: FlightPose = FlightPose {
    palm_roll_deg: [90.0; 2],
    ..FLIGHT
};

struct History {
    state: FlightState,
    calibration: Calibration,
    sample: GestureSample,
}

impl History {
    fn new() -> Self {
        Self {
            state: FlightState::default(),
            calibration: calibration(),
            sample: GestureSample {
                epoch: 1,
                sequence: 0,
                time_ns: 0,
            },
        }
    }

    fn observe(&mut self, pose: FlightPose) -> GestureCommands {
        self.sample.sequence += 1;
        self.sample.time_ns += 62_500_000;
        self.repeat(pose)
    }

    fn repeat(&mut self, pose: FlightPose) -> GestureCommands {
        self.state.calibrate(self.calibration);
        self.state.gestures.step(
            GestureInput {
                sample: self.sample,
                mode: SUIT,
                airborne: false,
                deploy_grapple: false,
                gameplay: true,
                canopy_requested: false,
                pose: Some(pose),
                claims: [HandClaims::default(); 2],
            },
            &CONFIG,
        )
    }

    fn hold(&mut self, pose: FlightPose, count: usize) -> Vec<GestureCommands> {
        (0..count).map(|_| self.observe(pose)).collect()
    }

    fn arm(&mut self) {
        assert_idle(&self.hold(LOWERED, 3));
        assert_idle(&self.hold(FLIGHT, 5));
    }

    fn braking(&mut self) -> GestureCommands {
        self.arm();
        let outputs = self.hold(BRAKE, 3);
        let command = *outputs.last().unwrap();
        assert!(command.brake);
        command
    }
}

fn calibration() -> Calibration {
    Calibration {
        grip_offset: GripOffset::INDEX_BEST_GUESS,
        dimensions: BodyDimensions::default(),
    }
}

fn next_bit(value: &mut f32) {
    *value = f32::from_bits(value.to_bits() + 1);
}

fn edits() -> Vec<Calibration> {
    (0..15)
        .map(|field| {
            let mut changed = calibration();
            match field {
                0 => next_bit(&mut changed.grip_offset.position_m.x),
                1 => next_bit(&mut changed.grip_offset.position_m.y),
                2 => next_bit(&mut changed.grip_offset.position_m.z),
                3 => next_bit(&mut changed.grip_offset.rotation_deg.x),
                4 => next_bit(&mut changed.grip_offset.rotation_deg.y),
                5 => next_bit(&mut changed.grip_offset.rotation_deg.z),
                6 => next_bit(&mut changed.dimensions.height),
                7 => next_bit(&mut changed.dimensions.eye_height),
                8 => next_bit(&mut changed.dimensions.shoulder_height),
                9 => next_bit(&mut changed.dimensions.shoulder_width),
                10 => next_bit(&mut changed.dimensions.arm_span),
                11 => next_bit(&mut changed.dimensions.upper_arm),
                12 => next_bit(&mut changed.dimensions.forearm),
                13 => next_bit(&mut changed.dimensions.hand),
                14 => changed.dimensions.seated_eye_height = Some(1.2),
                _ => unreachable!(),
            }
            assert!(changed.dimensions.validate().is_ok());
            changed
        })
        .collect()
}

fn assert_idle(commands: &[GestureCommands]) {
    assert!(
        commands
            .iter()
            .all(|command| *command == GestureCommands::default()),
        "{commands:?}"
    );
}

#[test]
fn every_calibration_component_compares_exact_bits() {
    let original = calibration();
    assert!(original.same_bits(original));
    for changed in edits() {
        assert!(!original.same_bits(changed));
        assert!(changed.same_bits(changed));
    }
    let mut negative_zero = original;
    negative_zero.grip_offset.position_m.z = -0.0;
    assert!(!original.same_bits(negative_zero));
    let mut seated = original;
    seated.dimensions.seated_eye_height = Some(1.2);
    let mut changed_seated = seated;
    next_bit(
        changed_seated
            .dimensions
            .seated_eye_height
            .as_mut()
            .unwrap(),
    );
    assert!(!seated.same_bits(changed_seated));
}

#[test]
fn calibration_edits_disarm_flight_and_brake_without_reusing_the_packet() {
    for changed in edits() {
        for braking in [false, true] {
            let mut h = History::new();
            let last = if braking {
                h.braking();
                BRAKE
            } else {
                h.arm();
                FLIGHT
            };
            h.calibration = changed;
            assert_eq!(h.repeat(last), GestureCommands::default());
            assert_eq!(h.repeat(BRAKE), GestureCommands::default());
            // The held brake pose takes the brake's whole dwell again, from the next packet.
            let resumed = h.hold(BRAKE, 3);
            assert_idle(&resumed[..2]);
            assert!(resumed[2].brake);
            assert_idle(&h.hold(FLIGHT, 5));
            assert_idle(&h.hold(BRAKE, 2));
            assert!(h.observe(BRAKE).brake);
        }
    }
}

#[test]
fn calibration_edits_discard_partial_flight() {
    for changed in edits() {
        let mut h = History::new();
        assert_idle(&h.hold(FLIGHT, 3));
        h.calibration = changed;
        assert_eq!(h.repeat(FLIGHT), GestureCommands::default());
        assert_idle(&h.hold(FLIGHT, 4));
        assert_eq!(h.state.gestures.phase(), GesturePhase::AwaitingFlight);
        assert_idle(&h.hold(FLIGHT, 1));
        assert_eq!(h.state.gestures.phase(), GesturePhase::Armed);
        assert_idle(&h.hold(BRAKE, 2));
        assert!(h.observe(BRAKE).brake);
    }
}

#[test]
fn calibration_edits_discard_partial_close_until_fresh_neutral_and_flight() {
    for changed in edits() {
        let mut h = History::new();
        h.arm();
        assert_idle(&h.hold(LOWERED, 3));
        h.calibration = changed;
        assert_eq!(h.repeat(LOWERED), GestureCommands::default());
        assert_idle(&h.hold(LOWERED, 6));
        assert_idle(&h.hold(FLIGHT, 5));
        let outputs = h.hold(LOWERED, 6);
        assert_eq!(
            outputs
                .iter()
                .filter(|command| command.transition == Some(SuitTransition::Close))
                .count(),
            1
        );
        assert_idle(&h.hold(LOWERED, 6));
    }
}

fn commit_history(
    history: &mut History,
    commands: GestureCommands,
    claims: [HandClaims; 2],
    desired: &mut Vec<Desired>,
) {
    commit_input(
        &mut history.state,
        commands,
        Some(GestureInput {
            sample: history.sample,
            mode: SUIT,
            airborne: false,
            deploy_grapple: false,
            gameplay: true,
            canopy_requested: canopy(desired),
            pose: Some(BRAKE),
            claims,
        }),
        &CONFIG,
        desired,
    )
}

#[test]
fn late_canopy_and_new_claims_cancel_otherwise_publishable_braking_and_reset_history() {
    for cancellation in 0..9 {
        let mut h = History::new();
        let candidate = h.braking();
        let mut baseline = Vec::new();
        commit_history(&mut h, candidate, [HandClaims::default(); 2], &mut baseline);
        assert_eq!(
            baseline,
            [CANCEL, WINGSUIT_AIRBRAKE].map(|action| Desired {
                target: Target::player(action),
                want: Want::Hold(1.0),
            })
        );

        let mut claims = [HandClaims::default(); 2];
        let mut desired = Vec::new();
        if cancellation == 0 {
            desired.push(Desired {
                target: Target::player(OPEN_PARACHUTE),
                want: Want::Hold(1.0),
            });
        } else {
            let claim = &mut claims[(cancellation - 1) / 4];
            match (cancellation - 1) % 4 {
                0 => claim.grip = true,
                1 => claim.slot = true,
                2 => claim.mechanical = true,
                3 => claim.grapple = true,
                _ => unreachable!(),
            }
        }
        let expected = desired.clone();
        commit_history(&mut h, candidate, claims, &mut desired);
        assert_eq!(desired, expected);
        assert_eq!(h.repeat(BRAKE), GestureCommands::default());
        // The cancellation drops the brake; the held pose takes the brake's dwell again.
        let resumed = h.hold(BRAKE, 3);
        assert_idle(&resumed[..2]);
        assert!(resumed[2].brake);
    }
}

#[test]
fn late_canopy_suppresses_an_otherwise_publishable_close() {
    let mut h = History::new();
    h.arm();
    let outputs = h.hold(LOWERED, 5);
    let candidate = *outputs.last().unwrap();
    assert_eq!(candidate.transition, Some(SuitTransition::Close));
    let mut baseline = Vec::new();
    commit_history(&mut h, candidate, [HandClaims::default(); 2], &mut baseline);
    assert_eq!(
        baseline,
        vec![Desired {
            target: Target::player(OPEN_WINGSUIT),
            want: Want::Click
        }]
    );
    let canopy = Desired {
        target: Target::player(OPEN_PARACHUTE),
        want: Want::Hold(1.0),
    };
    let mut desired = vec![canopy];
    commit_history(&mut h, candidate, [HandClaims::default(); 2], &mut desired);
    assert_eq!(desired, vec![canopy]);
    assert_eq!(h.repeat(LOWERED), GestureCommands::default());
    assert_idle(&h.hold(LOWERED, 6));
}

#[test]
fn missing_packets_clear_pose_history_and_a_lowered_pose_does_not_deploy() {
    let _guard = flight_pose::test_guard();
    let frame = ControllerFrame {
        sequence: 1,
        display_time_ns: 1,
        published_at: Instant::now(),
        focused: true,
        action_set: ActionSetKind::Wingsuit,
        hands: Default::default(),
    };
    let config = ControllersConfig::default();
    let dimensions = BodyDimensions::default();
    let tick = Tick {
        frame: &frame,
        config: &config,
        dimensions: &dimensions,
        detection: Detection {
            mode: SUIT,
            ..Default::default()
        },
        airborne: false,
        ready: true,
        ragdoll: false,
        grapple: false,
        grabs: [None; 2],
        reaches: [&[], &[]],
        dt: 0.01,
    };
    let mut h = History::new();
    h.braking();
    assert_eq!(
        evaluate(&mut h.state, &tick, &[]),
        GestureCommands::default()
    );
    assert_eq!(h.repeat(BRAKE), GestureCommands::default());

    for loss in 0..3 {
        for cancelled in [false, true] {
            let mut state = FlightState::default();
            let falling = Mode::Airborne(AirborneVariant::Falling);
            let observation = |sequence| GestureInput {
                sample: GestureSample {
                    epoch: 1,
                    sequence,
                    time_ns: sequence as i64 * 62_500_000,
                },
                mode: falling,
                airborne: true,
                deploy_grapple: false,
                gameplay: true,
                canopy_requested: false,
                pose: Some(FLIGHT),
                claims: [HandClaims::default(); 2],
            };
            for sequence in 1..=5 {
                let commands = evaluate_input(
                    &mut state,
                    calibration(),
                    Some(observation(sequence)),
                    &CONFIG,
                );
                assert_eq!(
                    commands.transition,
                    (sequence == 5).then_some(SuitTransition::Deploy)
                );
            }
            let mut config = ControllersConfig::default();
            config.hands.grip = calibration().grip_offset;
            config.vehicles.wingsuit_gestures = CONFIG;
            let missing = Tick {
                config: &config,
                detection: Detection {
                    mode: falling,
                    ..Default::default()
                },
                airborne: true,
                grabs: [cancelled.then_some(GrabVolume::Foregrip), None],
                ..tick
            };
            if loss == 0 {
                assert_eq!(
                    evaluate(&mut state, &missing, &[]),
                    GestureCommands::default()
                );
                assert_eq!(state.current(), GestureAvailability::Unavailable);
                commit(
                    &mut state,
                    GestureCommands::default(),
                    &missing,
                    &mut Vec::new(),
                );
            } else {
                let absent = GestureInput {
                    pose: None,
                    claims: [
                        HandClaims {
                            grip: cancelled,
                            ..Default::default()
                        },
                        HandClaims::default(),
                    ],
                    ..observation(if loss == 1 { 6 } else { 5 })
                };
                if loss == 1 {
                    assert_eq!(
                        evaluate_input(&mut state, calibration(), Some(absent), &CONFIG),
                        GestureCommands::default()
                    );
                }
                commit_input(
                    &mut state,
                    GestureCommands::default(),
                    Some(absent),
                    &CONFIG,
                    &mut Vec::new(),
                );
            }
            let recovered = GestureInput {
                pose: Some(LOWERED),
                ..observation(13)
            };
            let commands = evaluate_input(&mut state, calibration(), Some(recovered), &CONFIG);
            assert_eq!(
                commands.transition, None,
                "a lowered pose does not deploy: loss {loss}, cancelled {cancelled}"
            );
            let flying = GestureInput {
                pose: Some(FLIGHT),
                ..observation(17)
            };
            let commands = evaluate_input(&mut state, calibration(), Some(flying), &CONFIG);
            assert_eq!(
                commands.transition,
                (!cancelled).then_some(SuitTransition::Deploy),
                "the pending retry follows the flight pose: loss {loss}, cancelled {cancelled}"
            );
        }
    }
}

/// A hand at a hip socket with its grip open is a lowered arm, which the neutral and the closure
/// are made of; only the grip squeezed there, a draw starting, claims the hand.
#[test]
fn a_body_slot_claims_the_hand_only_while_its_grip_is_squeezed() {
    let _guard = flight_pose::test_guard();
    let config = ControllersConfig::default();
    let dimensions = BodyDimensions::default();
    let hips = [
        GrabVolume::Socket(Socket::Hip(Hand::Left)),
        GrabVolume::Socket(Socket::Hip(Hand::Right)),
    ];
    for squeezed in [false, true] {
        let mut frame = ControllerFrame {
            sequence: 1,
            display_time_ns: 1,
            published_at: Instant::now(),
            focused: true,
            action_set: ActionSetKind::OnFoot,
            hands: Default::default(),
        };
        frame.hands[Hand::Right.index()]
            .buttons
            .set(Button::SqueezePress, squeezed);
        let tick = Tick {
            frame: &frame,
            config: &config,
            dimensions: &dimensions,
            detection: Detection {
                mode: Mode::Airborne(AirborneVariant::Falling),
                ..Default::default()
            },
            airborne: false,
            ready: true,
            ragdoll: false,
            grapple: false,
            grabs: [None; 2],
            reaches: [&hips[..1], &hips[1..]],
            dt: 0.01,
        };
        let claims = claims(&tick, Instant::now());
        assert!(!claims[Hand::Left.index()].slot, "the left grip is open");
        assert_eq!(claims[Hand::Right.index()].slot, squeezed);
    }
}

#[test]
fn the_line_pose_and_a_flat_palm_yield_but_physical_grips_still_claim_the_hand() {
    assert!(!claims_gesture(GripSource::GrappleLine));
    assert!(!claims_gesture(GripSource::Surface));
    for source in [
        GripSource::Magazine,
        GripSource::Foregrip,
        GripSource::Climb,
        GripSource::Door,
        GripSource::ParachuteHandle,
    ] {
        assert!(claims_gesture(source), "{source:?}");
    }
}

#[test]
fn a_held_item_rides_along_and_a_body_slot_in_reach_claims_the_hand() {
    for source in [
        GripSource::Gun,
        GripSource::Grenade,
        GripSource::C4,
        GripSource::Pickup,
    ] {
        assert!(holds_item(source), "{source:?}");
    }
    for source in [
        GripSource::Magazine,
        GripSource::Foregrip,
        GripSource::Climb,
        GripSource::Door,
        GripSource::ParachuteHandle,
    ] {
        assert!(!holds_item(source), "{source:?}");
    }
    for volume in [
        GrabVolume::GrenadePouch,
        GrabVolume::MagazinePouch,
        GrabVolume::C4Pouch,
    ] {
        assert!(volume.on_body(), "{volume:?}");
    }
    for volume in [GrabVolume::Foregrip, GrabVolume::Handle, GrabVolume::Climb] {
        assert!(!volume.on_body(), "{volume:?}");
    }
}
