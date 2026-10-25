mod current;

use jc3gi::input::input_action_map::Action::{CANCEL, OPEN_WINGSUIT, WINGSUIT_AIRBRAKE};

use super::*;
use crate::controllers::{
    bindings::hold::{Target, Want},
    frame::ActionSetKind,
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
    palm_forward: [0.0; 2],
};
const LOWERED: FlightPose = FlightPose {
    lateral: [0.03; 2],
    reach: [0.33; 2],
    height: [-0.30; 2],
    angle_deg: [90.0; 2],
    palm_forward: [0.0; 2],
};
const BRAKE: FlightPose = FlightPose {
    palm_forward: [1.0; 2],
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
                gameplay: true,
                canopy_requested: false,
                pose: Some(pose),
                occupied: [false; 2],
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
        assert!(command.brake && command.reserve_pitch);
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
            assert_idle(&h.hold(BRAKE, 6));
            assert_idle(&h.hold(FLIGHT, 6));
            assert_idle(&h.hold(LOWERED, 6));
            assert_idle(&h.hold(FLIGHT, 5));
            assert!(h.observe(BRAKE).reserve_pitch);
            assert!(h.hold(BRAKE, 2).last().unwrap().brake);
        }
    }
}

#[test]
fn calibration_edits_discard_partial_neutral_without_rewinding_the_watermark() {
    for changed in edits() {
        let mut h = History::new();
        assert_idle(&h.hold(LOWERED, 2));
        h.calibration = changed;
        for _ in 0..4 {
            assert_eq!(h.repeat(LOWERED), GestureCommands::default());
        }
        assert_idle(&h.hold(LOWERED, 2));
        assert_idle(&h.hold(FLIGHT, 5));
        assert_idle(&h.hold(BRAKE, 3));
        h.arm();
        assert!(h.observe(BRAKE).reserve_pitch);
    }
}

#[test]
fn calibration_edits_discard_partial_flight() {
    for changed in edits() {
        let mut h = History::new();
        assert_idle(&h.hold(LOWERED, 3));
        assert_idle(&h.hold(FLIGHT, 3));
        h.calibration = changed;
        assert_eq!(h.repeat(FLIGHT), GestureCommands::default());
        assert_idle(&h.hold(FLIGHT, 5));
        assert_idle(&h.hold(BRAKE, 3));
        h.arm();
        assert!(h.observe(BRAKE).reserve_pitch);
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
) -> bool {
    commit_input(
        &mut history.state,
        commands,
        Some(GestureInput {
            sample: history.sample,
            mode: SUIT,
            gameplay: true,
            canopy_requested: canopy(desired),
            pose: Some(BRAKE),
            occupied: [false; 2],
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
        assert!(commit_history(
            &mut h,
            candidate,
            [HandClaims::default(); 2],
            &mut baseline
        ));
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
                1 => claim.trigger = true,
                2 => claim.mechanical = true,
                3 => claim.grapple = true,
                _ => unreachable!(),
            }
        }
        let expected = desired.clone();
        assert!(!commit_history(&mut h, candidate, claims, &mut desired));
        assert_eq!(desired, expected);
        assert_eq!(h.repeat(BRAKE), GestureCommands::default());
        assert_idle(&h.hold(BRAKE, 6));
        h.arm();
        assert!(h.observe(BRAKE).reserve_pitch);
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
    assert!(!commit_history(
        &mut h,
        candidate,
        [HandClaims::default(); 2],
        &mut baseline
    ));
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
    assert!(!commit_history(
        &mut h,
        candidate,
        [HandClaims::default(); 2],
        &mut desired
    ));
    assert_eq!(desired, vec![canopy]);
    assert_eq!(h.repeat(LOWERED), GestureCommands::default());
    assert_idle(&h.hold(LOWERED, 6));
}

#[test]
fn missing_packet_still_resets_the_wrapper_without_a_default_frame_fixture() {
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
        ready: true,
        ragdoll: false,
        grapple: false,
        grabs: [None; 2],
        dt: 0.01,
    };
    let mut h = History::new();
    h.braking();
    assert_eq!(
        evaluate(&mut h.state, &tick, &[]),
        GestureCommands::default()
    );
    assert_eq!(h.repeat(BRAKE), GestureCommands::default());
}
