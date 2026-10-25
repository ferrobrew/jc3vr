use std::time::Duration;

use glam::{Quat, Vec3};
use openxr as xr;

use super::*;
use crate::{
    controllers::{
        frame::{HandFrame, Pose, TrackedPose},
        mode::{OnFootVariant, VehicleClass, VehicleSeat},
    },
    vr::input::flight_pose::LocatedHead,
};

fn input(mode: Mode) -> GestureInput {
    GestureInput {
        sample: GestureSample {
            epoch: 1,
            sequence: 1,
            time_ns: 1,
        },
        mode,
        gameplay: true,
        canopy_requested: false,
        pose: Some(FLIGHT),
        occupied: [false; 2],
        claims: [HandClaims::default(); 2],
    }
}

#[test]
fn evaluation_and_commit_retain_current_availability_without_emitting_actions() {
    for (mode, expected) in [
        (
            Mode::Airborne(AirborneVariant::Falling),
            GestureAvailability::Deploy,
        ),
        (SUIT, GestureAvailability::CloseAndBrake),
    ] {
        let mut state = FlightState::default();
        for sequence in 1..20 {
            let input = GestureInput {
                sample: GestureSample {
                    sequence,
                    time_ns: sequence as i64 * 50_000_000,
                    ..input(mode).sample
                },
                ..input(mode)
            };
            let commands = evaluate_input(&mut state, calibration(), Some(input), &CONFIG);
            assert_eq!(commands, GestureCommands::default());
            let mut desired = Vec::new();
            assert!(!commit_input(
                &mut state,
                commands,
                Some(input),
                &CONFIG,
                &mut desired
            ));
            assert!(desired.is_empty());
            assert_eq!(state.current(), expected);
        }
    }
}

#[test]
fn late_input_changes_remove_current_and_cancel_pending_commands() {
    for denial in 0..21 {
        let mut history = History::new();
        history.braking();
        let mut state = history.state;
        let original = GestureInput {
            sample: history.sample,
            pose: Some(BRAKE),
            ..input(SUIT)
        };
        let commands = evaluate_input(&mut state, calibration(), Some(original), &CONFIG);
        assert!(commands.brake && commands.reserve_pitch);
        assert_eq!(state.current(), GestureAvailability::CloseAndBrake);
        let mut late = Some(original);
        let mut config = CONFIG;
        match denial {
            0 => late = None,
            1 => late.as_mut().unwrap().sample.sequence += 1,
            2 => late.as_mut().unwrap().pose = None,
            3 => late.as_mut().unwrap().pose.as_mut().unwrap().reach[0] = f32::NAN,
            4 => late.as_mut().unwrap().gameplay = false,
            5 => late.as_mut().unwrap().canopy_requested = true,
            6 | 7 => late.as_mut().unwrap().occupied[denial - 6] = true,
            8..=15 => {
                let claim = &mut late.as_mut().unwrap().claims[(denial - 8) / 4];
                match (denial - 8) % 4 {
                    0 => claim.grip = true,
                    1 => claim.trigger = true,
                    2 => claim.mechanical = true,
                    _ => claim.grapple = true,
                }
            }
            16 => config.enabled = false,
            17 => config.flight_dwell_s = f32::NAN,
            18 => late.as_mut().unwrap().mode = Mode::Airborne(AirborneVariant::Falling),
            19 => late.as_mut().unwrap().sample.epoch += 1,
            _ => late.as_mut().unwrap().sample.time_ns += 1,
        }
        let mut desired = Vec::new();
        assert!(
            !commit_input(&mut state, commands, late, &config, &mut desired),
            "{denial}"
        );
        assert!(desired.is_empty(), "{denial}");
        assert_eq!(
            state.current(),
            GestureAvailability::Unavailable,
            "{denial}"
        );
    }
}

#[test]
fn evaluation_denies_missing_disabled_invalid_and_unsupported_inputs() {
    for denial in 0..10 {
        let mut state = FlightState::default();
        let mut observation = Some(input(SUIT));
        let mut config = CONFIG;
        match denial {
            0 => observation = None,
            1 => config.enabled = false,
            2 => config.close_dwell_s = f32::NAN,
            3 => observation.as_mut().unwrap().pose = None,
            4 => {
                observation
                    .as_mut()
                    .unwrap()
                    .pose
                    .as_mut()
                    .unwrap()
                    .palm_forward[0] = 2.0
            }
            5 => observation.as_mut().unwrap().mode = Mode::Ui,
            6 => observation.as_mut().unwrap().mode = Mode::Airborne(AirborneVariant::Parachute),
            7 => observation.as_mut().unwrap().mode = Mode::OnFoot(OnFootVariant::Ground),
            8 => {
                observation.as_mut().unwrap().mode = Mode::Vehicle(VehicleSeat {
                    class: VehicleClass::Plane,
                    driver: true,
                })
            }
            _ => observation.as_mut().unwrap().canopy_requested = true,
        }
        assert_eq!(
            evaluate_input(&mut state, calibration(), observation, &config),
            GestureCommands::default(),
        );
        assert_eq!(
            state.current(),
            GestureAvailability::Unavailable,
            "{denial}"
        );
    }
}

#[test]
fn actual_packet_evaluate_and_commit_apply_late_canopy_staleness_and_claims() {
    let _guard = flight_pose::test_guard();
    let now = Instant::now();
    let eyes = [xr::Posef {
        position: xr::Vector3f {
            x: 0.0,
            y: 1.6,
            z: 0.0,
        },
        ..xr::Posef::IDENTITY
    }; 2];
    let pending = flight_pose::begin_located(Some(LocatedHead {
        eyes,
        flags: xr::ViewStateFlags::POSITION_VALID
            | xr::ViewStateFlags::ORIENTATION_VALID
            | xr::ViewStateFlags::POSITION_TRACKED
            | xr::ViewStateFlags::ORIENTATION_TRACKED,
        display_time_ns: 9_123,
    }))
    .unwrap();
    let frame = ControllerFrame {
        sequence: 9_123,
        display_time_ns: 9_123,
        published_at: now,
        focused: true,
        action_set: ActionSetKind::Wingsuit,
        hands: Hand::BOTH.map(|hand| HandFrame {
            active: true,
            grip: TrackedPose {
                pose: Pose {
                    position: Vec3::new(if hand == Hand::Left { -0.45 } else { 0.45 }, 1.3, 0.0),
                    orientation: Quat::IDENTITY,
                },
                position_valid: true,
                orientation_valid: true,
                position_tracked: true,
                orientation_tracked: true,
                ..Default::default()
            },
            ..Default::default()
        }),
    };
    assert!(flight_pose::complete(
        pending,
        frame.clone(),
        (frame.sequence, now)
    ));
    let mut config = ControllersConfig::default();
    config.hands.grip = GripOffset {
        position_m: Vec3::ZERO,
        rotation_deg: Vec3::ZERO,
    };
    let dimensions = BodyDimensions::default();
    let mut tick = Tick {
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
    let mut state = FlightState::default();
    let commands = evaluate(&mut state, &tick, &[]);
    assert_eq!(commands, GestureCommands::default());
    assert_eq!(state.current(), GestureAvailability::CloseAndBrake);
    let mut desired = Vec::new();
    assert!(!commit(&mut state, commands, &tick, &mut desired));
    assert!(desired.is_empty());
    assert_eq!(state.current(), GestureAvailability::CloseAndBrake);

    tick.grabs[0] = Some(GrabVolume::Foregrip);
    assert!(!commit(&mut state, commands, &tick, &mut desired));
    assert_eq!(state.current(), GestureAvailability::Unavailable);
    tick.grabs = [None; 2];
    let commands = evaluate(&mut state, &tick, &[]);
    assert_eq!(state.current(), GestureAvailability::CloseAndBrake);
    desired.push(Desired {
        target: Target::player(OPEN_PARACHUTE),
        want: Want::Hold(1.0),
    });
    let expected = desired.clone();
    assert!(!commit(&mut state, commands, &tick, &mut desired));
    assert_eq!(desired, expected);
    assert_eq!(state.current(), GestureAvailability::Unavailable);

    desired.clear();
    for invalid in [false, true] {
        let mut changed = config;
        if invalid {
            changed.vehicles.wingsuit_gestures.flight_dwell_s = f32::NAN;
        } else {
            changed.vehicles.wingsuit_gestures.enabled = false;
        }
        let changed_tick = Tick {
            config: &changed,
            ..tick
        };
        let commands = evaluate(&mut state, &changed_tick, &[]);
        assert_eq!(commands, GestureCommands::default());
        assert_eq!(state.current(), GestureAvailability::Unavailable);
        assert!(!commit(&mut state, commands, &changed_tick, &mut desired));
        assert_eq!(state.current(), GestureAvailability::Unavailable);
    }
    let commands = evaluate(&mut state, &tick, &[]);
    assert_eq!(state.current(), GestureAvailability::CloseAndBrake);
    let expired_at = now - Duration::from_secs(10);
    let stale = ControllerFrame {
        published_at: expired_at,
        ..frame.clone()
    };
    let pending = flight_pose::begin_located(Some(LocatedHead {
        eyes,
        flags: xr::ViewStateFlags::POSITION_VALID
            | xr::ViewStateFlags::ORIENTATION_VALID
            | xr::ViewStateFlags::POSITION_TRACKED
            | xr::ViewStateFlags::ORIENTATION_TRACKED,
        display_time_ns: stale.display_time_ns,
    }))
    .unwrap();
    assert!(flight_pose::complete(
        pending,
        stale.clone(),
        (stale.sequence, expired_at)
    ));
    tick.frame = &stale;
    assert!(!commit(&mut state, commands, &tick, &mut desired));
    assert_eq!(state.current(), GestureAvailability::Unavailable);
    assert!(desired.is_empty());
    flight_pose::unavailable();
    assert_eq!(evaluate(&mut state, &tick, &[]), GestureCommands::default());
    assert!(!commit(
        &mut state,
        GestureCommands::default(),
        &tick,
        &mut desired
    ));
    assert_eq!(state.current(), GestureAvailability::Unavailable);
}
