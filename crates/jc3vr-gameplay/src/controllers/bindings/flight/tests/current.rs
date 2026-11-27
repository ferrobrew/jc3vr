use std::time::Duration;

use glam::{Quat, Vec3};
use jc3gi::input::input_action_map::Action::{
    ACCELERATE, CANCEL, FIRE_RIGHT, OPEN_WINGSUIT, RELEASE_GRAPPLE, VEHICLE_RELEASE_GRAPPLE,
};
use jc3vr_body::controllers::mode::{OnFootVariant, VehicleClass, VehicleSeat};
use jc3vr_input::frame::{HandFrame, Pose, TrackedPose};
use jc3vr_view::vr::input::flight_pose::LocatedHead;
use openxr as xr;

use super::*;

fn input(mode: Mode) -> GestureInput {
    GestureInput {
        sample: GestureSample {
            epoch: 1,
            sequence: 1,
            time_ns: 1,
        },
        mode,
        airborne: false,
        deploy_grapple: false,
        gameplay: true,
        canopy_requested: false,
        pose: Some(FLIGHT),
        claims: [HandClaims::default(); 2],
    }
}

#[test]
fn grapple_traversal_arbitration_yields_reels_and_hangs_but_not_seats() {
    for mode in [
        Mode::OnFoot(OnFootVariant::Ground),
        Mode::Airborne(AirborneVariant::Falling),
        SUIT,
        Mode::Airborne(AirborneVariant::Parachute),
        Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Car,
            driver: true,
        }),
    ] {
        for traversal in [
            None,
            Some(Traversal::PreReel),
            Some(Traversal::ReelingIn),
            Some(Traversal::PreHang),
            Some(Traversal::ReeledIn),
        ] {
            for airborne in [false, true] {
                for grapple in [false, true] {
                    let detection = Detection {
                        mode,
                        traversal,
                        ..Default::default()
                    };
                    let expected = grapple
                        && match traversal {
                            None => {
                                mode == Mode::Airborne(AirborneVariant::Falling)
                                    || (mode == Mode::OnFoot(OnFootVariant::Ground) && airborne)
                            }
                            Some(Traversal::PreReel | Traversal::ReelingIn) => matches!(
                                mode,
                                Mode::OnFoot(OnFootVariant::Ground)
                                    | Mode::Airborne(AirborneVariant::Falling)
                            ),
                            Some(Traversal::PreHang | Traversal::ReeledIn) => matches!(
                                mode,
                                Mode::OnFoot(OnFootVariant::Ground)
                                    | Mode::Airborne(
                                        AirborneVariant::Falling | AirborneVariant::Parachute
                                    )
                            ),
                        };
                    assert_eq!(deploy_grapple(detection, airborne, grapple), expected);
                    assert_eq!(
                        grapple_claim(detection, airborne, grapple),
                        (grapple || traversal.is_some()) && !expected
                    );
                }
            }
        }
    }
}

#[test]
fn grapple_deployment_retracts_only_the_active_wire_without_releasing_dual_tethers() {
    for denial in 0..8 {
        let mut state = FlightState::default();
        let mut observation = input(Mode::OnFoot(OnFootVariant::Ground));
        observation.deploy_grapple = true;
        let mut commands = GestureCommands::default();
        for sequence in 1..=5 {
            observation.sample.sequence = sequence;
            observation.sample.time_ns = sequence as i64 * 62_500_000;
            commands = evaluate_input(&mut state, calibration(), Some(observation), &CONFIG);
        }
        assert_eq!(commands.transition, Some(SuitTransition::Deploy));
        let mut desired: Vec<_> = [
            FIRE_GRAPPLE,
            RETRACT_GRAPPLE,
            PUSH_GRAPPLE,
            FIRE_RIGHT,
            ACCELERATE,
        ]
        .map(|action| Desired {
            target: Target::player(action),
            want: Want::Hold(1.0),
        })
        .into();
        if denial == 1 {
            desired.push(Desired {
                target: Target::player(OPEN_PARACHUTE),
                want: Want::Click,
            });
        } else if denial > 1 {
            let claim = &mut observation.claims[(denial - 2) / 3];
            match (denial - 2) % 3 {
                0 => claim.grip = true,
                1 => claim.slot = true,
                _ => claim.mechanical = true,
            }
        }
        let before = desired.clone();
        commit_input(
            &mut state,
            commands,
            Some(observation),
            &CONFIG,
            &mut desired,
        );
        if denial == 0 {
            assert_eq!(&desired[..2], &before[3..]);
            assert_eq!(desired[2].target.action, OPEN_WINGSUIT);
            assert_eq!(desired[2].want, Want::Click);
            let mut active_wire = true;
            retract_committed_wire(&mut state, || active_wire = false);
            assert!(!active_wire);
            retract_committed_wire(&mut state, || panic!("the retract must be consumed once"));
            assert!(!desired.iter().any(|input| matches!(
                input.target.action,
                RELEASE_GRAPPLE | VEHICLE_RELEASE_GRAPPLE
            )));
            observation.mode = SUIT;
            observation.deploy_grapple = false;
            commit_input(
                &mut state,
                commands,
                Some(observation),
                &CONFIG,
                &mut desired,
            );
            retract_committed_wire(&mut state, || {
                panic!("suit acknowledgement cannot retract again")
            });
            assert_eq!(desired.len(), 3);
        } else {
            assert_eq!(desired, before, "denial {denial}");
            retract_committed_wire(&mut state, || {
                panic!("a rejected deployment cannot retract")
            });
        }
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
        (
            Mode::Airborne(AirborneVariant::Parachute),
            GestureAvailability::Deploy,
        ),
    ] {
        let mut state = FlightState::default();
        for sequence in 1..20 {
            let input = GestureInput {
                sample: GestureSample {
                    sequence,
                    time_ns: sequence as i64 * 50_000_000,
                    ..input(mode).sample
                },
                // Lowered arms, so a fall's start does not deploy the suit.
                pose: Some(LOWERED),
                ..input(mode)
            };
            let commands = evaluate_input(&mut state, calibration(), Some(input), &CONFIG);
            assert_eq!(commands, GestureCommands::default());
            let mut desired = Vec::new();
            commit_input(&mut state, commands, Some(input), &CONFIG, &mut desired);
            assert!(desired.is_empty());
            assert_eq!(state.current(), expected);
        }
    }
}

/// A flight pose under a canopy cuts the canopy; the suit's opening follows once the fall begins.
#[test]
fn a_canopy_flight_pose_cuts_the_canopy_before_the_suit() {
    let mut state = FlightState::default();
    let mut observation = input(Mode::Airborne(AirborneVariant::Parachute));
    let mut commands = GestureCommands::default();
    for sequence in 1..=5 {
        observation.sample.sequence = sequence;
        observation.sample.time_ns = sequence as i64 * 62_500_000;
        commands = evaluate_input(&mut state, calibration(), Some(observation), &CONFIG);
    }
    assert_eq!(commands.transition, Some(SuitTransition::Deploy));
    let mut desired = Vec::new();
    commit_input(
        &mut state,
        commands,
        Some(observation),
        &CONFIG,
        &mut desired,
    );
    assert_eq!(
        desired,
        vec![Desired {
            target: Target::player(CANCEL),
            want: Want::Click,
        }]
    );
    assert!(!state.retract_active_wire);
    // The fall out of the cut canopy takes the suit's opening act.
    observation.mode = Mode::Airborne(AirborneVariant::Falling);
    let mut deployed = false;
    for sequence in 6..=10 {
        observation.sample.sequence = sequence;
        observation.sample.time_ns = sequence as i64 * 62_500_000;
        let commands = evaluate_input(&mut state, calibration(), Some(observation), &CONFIG);
        if commands.transition != Some(SuitTransition::Deploy) {
            continue;
        }
        let mut desired = Vec::new();
        commit_input(
            &mut state,
            commands,
            Some(observation),
            &CONFIG,
            &mut desired,
        );
        assert_eq!(
            desired,
            vec![Desired {
                target: Target::player(OPEN_WINGSUIT),
                want: Want::Click,
            }]
        );
        deployed = true;
        break;
    }
    assert!(deployed, "the fall out of the canopy opens the suit");
}

/// A hang yields its line before the suit request: the grapple actions are dropped, the wire is
/// retracted once, and the suit's opening follows.
#[test]
fn a_hang_releases_the_line_before_the_suit_request() {
    let mut state = FlightState::default();
    let mut observation = input(Mode::OnFoot(OnFootVariant::Ground));
    observation.deploy_grapple = true;
    let mut commands = GestureCommands::default();
    for sequence in 1..=5 {
        observation.sample.sequence = sequence;
        observation.sample.time_ns = sequence as i64 * 62_500_000;
        commands = evaluate_input(&mut state, calibration(), Some(observation), &CONFIG);
    }
    assert_eq!(commands.transition, Some(SuitTransition::Deploy));
    let mut desired = [FIRE_GRAPPLE, RETRACT_GRAPPLE, PUSH_GRAPPLE]
        .map(|action| Desired {
            target: Target::player(action),
            want: Want::Hold(1.0),
        })
        .to_vec();
    commit_input(
        &mut state,
        commands,
        Some(observation),
        &CONFIG,
        &mut desired,
    );
    assert_eq!(
        desired,
        vec![Desired {
            target: Target::player(OPEN_WINGSUIT),
            want: Want::Click,
        }]
    );
    assert!(state.retract_active_wire);
    let mut retracted = false;
    retract_committed_wire(&mut state, || retracted = true);
    assert!(retracted);
    assert!(!state.retract_active_wire);
    // A retry in the deploy window does not retract a line the player fired since.
    commit_input(
        &mut state,
        GestureCommands {
            transition: Some(SuitTransition::Deploy),
            ..Default::default()
        },
        Some(observation),
        &CONFIG,
        &mut Vec::new(),
    );
    assert!(!state.retract_active_wire);
    let mut retried = false;
    retract_committed_wire(&mut state, || retried = true);
    assert!(!retried, "a retry does not retract a new line");
}

#[test]
fn late_input_changes_remove_current_and_cancel_pending_commands() {
    for denial in 0..19 {
        let mut history = History::new();
        history.braking();
        let mut state = history.state;
        let original = GestureInput {
            sample: history.sample,
            pose: Some(BRAKE),
            ..input(SUIT)
        };
        let commands = evaluate_input(&mut state, calibration(), Some(original), &CONFIG);
        assert!(commands.brake);
        assert_eq!(state.current(), GestureAvailability::CloseAndBrake);
        let mut late = Some(original);
        match denial {
            0 => late = None,
            1 => late.as_mut().unwrap().sample.sequence += 1,
            2 => late.as_mut().unwrap().pose = None,
            3 => late.as_mut().unwrap().pose.as_mut().unwrap().reach[0] = f32::NAN,
            4 => late.as_mut().unwrap().gameplay = false,
            5 => late.as_mut().unwrap().canopy_requested = true,
            6 | 7 => late.as_mut().unwrap().claims[denial - 6].grapple = true,
            8..=15 => {
                let claim = &mut late.as_mut().unwrap().claims[(denial - 8) / 4];
                match (denial - 8) % 4 {
                    0 => claim.grip = true,
                    1 => claim.slot = true,
                    2 => claim.mechanical = true,
                    _ => claim.grapple = true,
                }
            }
            16 => late.as_mut().unwrap().mode = Mode::Airborne(AirborneVariant::Falling),
            17 => late.as_mut().unwrap().sample.epoch += 1,
            _ => late.as_mut().unwrap().sample.time_ns += 1,
        }
        let mut desired = Vec::new();
        commit_input(&mut state, commands, late, &CONFIG, &mut desired);
        assert!(desired.is_empty(), "{denial}");
        assert_eq!(
            state.current(),
            GestureAvailability::Unavailable,
            "{denial}"
        );
    }
}

#[test]
fn evaluation_denies_missing_and_unsupported_inputs() {
    for denial in 0..8 {
        let mut state = FlightState::default();
        let mut observation = Some(input(SUIT));
        match denial {
            0 => observation = None,
            1 => observation.as_mut().unwrap().pose = None,
            2 => {
                observation
                    .as_mut()
                    .unwrap()
                    .pose
                    .as_mut()
                    .unwrap()
                    .palm_roll_deg[0] = 181.0
            }
            3 => observation.as_mut().unwrap().mode = Mode::Ui,
            4 => observation.as_mut().unwrap().mode = Mode::OnFoot(OnFootVariant::Swimming),
            5 => observation.as_mut().unwrap().mode = Mode::OnFoot(OnFootVariant::Ground),
            6 => {
                observation.as_mut().unwrap().mode = Mode::Vehicle(VehicleSeat {
                    class: VehicleClass::Plane,
                    driver: true,
                })
            }
            _ => observation.as_mut().unwrap().canopy_requested = true,
        }
        assert_eq!(
            evaluate_input(&mut state, calibration(), observation, &CONFIG),
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
        grip_offset: config.hands.grip,
        gestures: &jc3vr_vehicles::vehicles::tuning::VEHICLES
            .get()
            .wingsuit_gestures,
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
    let mut state = FlightState::default();
    let commands = evaluate(&mut state, &tick, &[]);
    assert_eq!(commands, GestureCommands::default());
    assert_eq!(state.current(), GestureAvailability::CloseAndBrake);
    let mut desired = Vec::new();
    commit(&mut state, commands, &tick, &mut desired);
    assert!(desired.is_empty());
    assert_eq!(state.current(), GestureAvailability::CloseAndBrake);

    tick.grabs[0] = Some(GrabVolume::Foregrip);
    commit(&mut state, commands, &tick, &mut desired);
    assert_eq!(state.current(), GestureAvailability::Unavailable);
    tick.grabs = [None; 2];
    let commands = evaluate(&mut state, &tick, &[]);
    assert_eq!(state.current(), GestureAvailability::CloseAndBrake);
    desired.push(Desired {
        target: Target::player(OPEN_PARACHUTE),
        want: Want::Hold(1.0),
    });
    let expected = desired.clone();
    commit(&mut state, commands, &tick, &mut desired);
    assert_eq!(desired, expected);
    assert_eq!(state.current(), GestureAvailability::Unavailable);

    desired.clear();
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
    commit(&mut state, commands, &tick, &mut desired);
    assert_eq!(state.current(), GestureAvailability::Unavailable);
    assert!(desired.is_empty());
    flight_pose::unavailable();
    assert_eq!(evaluate(&mut state, &tick, &[]), GestureCommands::default());
    commit(&mut state, GestureCommands::default(), &tick, &mut desired);
    assert_eq!(state.current(), GestureAvailability::Unavailable);
}
