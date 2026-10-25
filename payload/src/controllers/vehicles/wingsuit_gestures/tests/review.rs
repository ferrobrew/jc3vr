use std::time::Duration;

use super::*;

fn observation(sample: GestureSample, pose: Option<FlightPose>) -> GestureInput {
    GestureInput {
        sample,
        mode: SUIT,
        gameplay: true,
        canopy_requested: false,
        pose,
        occupied: [false; 2],
        claims: [HandClaims::default(); 2],
    }
}

fn measured(deltas: [Vec3; 2]) -> FlightPose {
    let shoulders = [Vec3::new(-0.20, 0.0, 0.0), Vec3::new(0.20, 0.0, 0.0)];
    FlightPose::measure(
        Hand::BOTH.map(|hand| Pose {
            position: shoulders[hand.index()] + deltas[hand.index()],
            orientation: Quat::IDENTITY,
        }),
        shoulders,
        Quat::IDENTITY,
        1.80,
    )
    .unwrap()
}

#[test]
fn realistic_straight_down_arms_establish_neutral_and_close_once_only_after_flight() {
    let down = measured([Vec3::new(0.0, -0.60, 0.0); 2]);
    assert!(
        down.reach
            .into_iter()
            .all(|reach| reach > CONFIG.withdraw_reach_span)
    );
    assert!(down.withdrawn(&CONFIG));
    let flight = measured([Vec3::new(-0.60, 0.0, 0.0), Vec3::new(0.60, 0.0, 0.0)]);
    let one_down = measured([Vec3::new(0.0, -0.60, 0.0), Vec3::new(0.60, 0.0, 0.0)]);
    let mut h = History::new(SUIT);
    assert!(
        h.hold(down, 20)
            .iter()
            .all(|command| *command == GestureCommands::default())
    );
    assert!(h.state.ready && !h.state.armed);
    h.hold(flight, 6);
    assert!(h.state.armed);
    assert!(
        h.hold(one_down, 20)
            .iter()
            .all(|command| command.transition.is_none())
    );
    let close = h.hold(down, 20);
    assert_eq!(
        close
            .iter()
            .filter(|command| command.transition == Some(SuitTransition::Close))
            .count(),
        1
    );
    assert!(
        h.hold(down, 20)
            .iter()
            .all(|command| command.transition.is_none())
    );
    let mut falling = History::new(FALLING);
    falling.hold(down, 5);
    let deploy = falling.hold(flight, 6);
    assert_eq!(
        deploy
            .iter()
            .filter(|command| command.transition == Some(SuitTransition::Deploy))
            .count(),
        1
    );
}

#[test]
fn withdrawn_geometry_excludes_banking_deadzone_and_crossed_wrists() {
    let bank = measured([Vec3::new(-0.50, 0.20, 0.0), Vec3::new(0.50, -0.20, 0.0)]);
    let gap = measured([Vec3::new(-0.28, -0.40, 0.0), Vec3::new(0.28, -0.40, 0.0)]);
    let crossed = measured([Vec3::new(0.45, -0.60, 0.0), Vec3::new(-0.45, -0.60, 0.0)]);
    let raised = measured([Vec3::new(0.0, 0.60, 0.0); 2]);
    for pose in [bank, gap, crossed, raised] {
        assert!(!pose.withdrawn(&CONFIG));
        let mut h = History::new(SUIT);
        h.arm();
        assert!(
            h.hold(pose, 20)
                .iter()
                .all(|command| command.transition.is_none())
        );
    }
}

#[derive(Debug, Copy, Clone)]
enum Interruption {
    Reset,
    Unavailable,
    Missing,
    Invalid,
    CanopyRequest,
    CanopyMode,
    Falling,
    Epoch,
    Gap,
    Occupied(Hand),
    Claim(Hand, HandClaims),
}

fn interrupt(h: &mut History, interruption: Interruption) {
    match interruption {
        Interruption::Reset => h.state.reset(),
        Interruption::Unavailable => {
            h.gameplay = false;
            assert_eq!(h.observe(Some(BRAKE)), GestureCommands::default());
            h.gameplay = true;
        }
        Interruption::Missing => {
            h.observe(None);
        }
        Interruption::Invalid => {
            h.observe(Some(FlightPose {
                height: [f32::NAN; 2],
                ..BRAKE
            }));
        }
        Interruption::CanopyRequest => {
            h.canopy = true;
            assert_eq!(h.observe(Some(BRAKE)), GestureCommands::default());
            h.canopy = false;
        }
        Interruption::CanopyMode => {
            h.mode = CANOPY;
            h.observe(Some(BRAKE));
            h.mode = SUIT;
        }
        Interruption::Falling => {
            h.mode = FALLING;
            h.observe(Some(BRAKE));
            h.mode = SUIT;
        }
        Interruption::Epoch => {
            h.sample.epoch += 1;
            h.sample.sequence = 0;
            h.sample.time_ns = 0;
        }
        Interruption::Gap => {
            h.sample.time_ns += 1_000_000_000;
        }
        Interruption::Occupied(hand) => {
            h.occupied[hand.index()] = true;
            assert_eq!(h.observe(Some(BRAKE)), GestureCommands::default());
            h.occupied = [false; 2];
        }
        Interruption::Claim(hand, claim) => {
            h.claims[hand.index()] = claim;
            assert_eq!(h.observe(Some(BRAKE)), GestureCommands::default());
            h.claims = [HandClaims::default(); 2];
        }
    }
}

#[test]
fn brake_entry_and_release_require_fresh_arming_after_every_interruption() {
    let mut interruptions = vec![
        Interruption::Reset,
        Interruption::Unavailable,
        Interruption::Missing,
        Interruption::Invalid,
        Interruption::CanopyRequest,
        Interruption::CanopyMode,
        Interruption::Falling,
        Interruption::Epoch,
        Interruption::Gap,
    ];
    for hand in Hand::BOTH {
        interruptions.push(Interruption::Occupied(hand));
        for claim in [
            HandClaims {
                grip: true,
                ..Default::default()
            },
            HandClaims {
                trigger: true,
                ..Default::default()
            },
            HandClaims {
                mechanical: true,
                ..Default::default()
            },
            HandClaims {
                grapple: true,
                ..Default::default()
            },
        ] {
            interruptions.push(Interruption::Claim(hand, claim));
        }
    }
    let mut initial = History::new(SUIT);
    assert!(
        initial
            .hold(BRAKE, 20)
            .iter()
            .all(|command| *command == GestureCommands::default())
    );
    for interruption in interruptions {
        let mut h = History::new(SUIT);
        h.arm();
        assert!(h.hold(BRAKE, 6).iter().any(|command| command.brake));
        interrupt(&mut h, interruption);
        assert!(
            h.hold(BRAKE, 20)
                .iter()
                .all(|command| *command == GestureCommands::default()),
            "{interruption:?}"
        );
        h.arm();
        let entering = h.observe(Some(BRAKE));
        assert!(
            entering.reserve_pitch && !entering.brake,
            "{interruption:?}"
        );
        assert!(h.hold(BRAKE, 6).iter().any(|command| command.brake));
    }
}

#[test]
fn isolated_folded_and_flight_samples_do_not_receive_the_previous_interval() {
    let mut state = WingsuitGestures::default();
    for (sequence, time_ns, pose) in [
        (1, 0, FLIGHT),
        (2, 200_000_000, FOLDED),
        (3, 400_000_000, FLIGHT),
    ] {
        let mut input = observation(
            GestureSample {
                epoch: 1,
                sequence,
                time_ns,
            },
            Some(pose),
        );
        input.mode = FALLING;
        assert_eq!(state.step(input, &CONFIG), GestureCommands::default());
    }
    assert!(!state.ready && !state.armed);
    let mut h = History::new(SUIT);
    h.arm();
    h.sample.time_ns += 150_000_000;
    assert!(h.observe(Some(FOLDED)).transition.is_none());
    assert!(h.state.close.since_ns == Some(h.sample.time_ns));
    h.observe(Some(FLIGHT));
    assert!(h.state.close.since_ns.is_none());
    h.sample.time_ns += 150_000_000;
    assert!(h.observe(Some(FOLDED)).transition.is_none());
}

#[test]
fn dwell_entry_boundaries_and_gaps_are_independent_of_the_poll_interval() {
    let mut dwell = Dwell::default();
    assert!(!dwell.update(false, 0, 0.125));
    assert!(!dwell.update(true, 200_000_000, 0.125));
    assert!(!dwell.update(true, 324_999_999, 0.125));
    assert!(dwell.update(true, 325_000_000, 0.125));
    assert!(!dwell.update(false, 400_000_000, 0.125));
    assert!(!dwell.update(true, 600_000_000, 0.125));
    let config = WingsuitGesturesConfig {
        neutral_dwell_s: 0.125,
        flight_dwell_s: 0.25,
        brake_dwell_s: 0.125,
        close_dwell_s: 0.25,
        max_sample_gap_s: 0.5,
        ..CONFIG
    };
    assert!(config.valid());
    let mut state = WingsuitGestures::default();
    let mut sample = GestureSample {
        epoch: 1,
        sequence: 1,
        time_ns: 0,
    };
    assert_eq!(
        state.step(observation(sample, Some(FOLDED)), &config),
        GestureCommands::default()
    );
    sample.sequence += 1;
    sample.time_ns = 125_000_000;
    state.step(observation(sample, Some(FOLDED)), &config);
    assert!(state.ready);
    sample.sequence += 1;
    sample.time_ns = 500_000_000;
    state.step(observation(sample, Some(FLIGHT)), &config);
    assert!(!state.armed);
    sample.sequence += 1;
    sample.time_ns = 750_000_000;
    state.step(observation(sample, Some(FLIGHT)), &config);
    assert!(state.armed);
    sample.sequence += 1;
    sample.time_ns = 1_250_000_000;
    let entry = state.step(observation(sample, Some(BRAKE)), &config);
    assert!(entry.reserve_pitch && !entry.brake);
    sample.sequence += 1;
    sample.time_ns = 1_375_000_000;
    assert!(state.step(observation(sample, Some(BRAKE)), &config).brake);
    sample.sequence += 1;
    sample.time_ns += 500_000_001;
    assert_eq!(
        state.step(observation(sample, Some(BRAKE)), &config),
        GestureCommands::default()
    );
    assert!(!state.armed);
}

#[test]
fn old_observations_cannot_lower_the_watermark_on_any_cancellation_path() {
    let high = GestureSample {
        epoch: 3,
        sequence: 100,
        time_ns: 1_000_000_000,
    };
    let old = GestureSample {
        epoch: 3,
        sequence: 97,
        time_ns: 850_000_000,
    };
    let mut paths = vec![observation(old, Some(FOLDED))];
    let mut unavailable = observation(old, Some(FOLDED));
    unavailable.gameplay = false;
    paths.push(unavailable);
    let mut canopy = observation(old, Some(FOLDED));
    canopy.canopy_requested = true;
    paths.push(canopy);
    paths.push(observation(old, None));
    paths.push(observation(
        old,
        Some(FlightPose {
            reach: [f32::NAN; 2],
            ..FOLDED
        }),
    ));
    for mode in [
        FALLING,
        CANOPY,
        Mode::Ui,
        Mode::OnFoot(OnFootVariant::Ground),
    ] {
        let mut input = observation(old, Some(FOLDED));
        input.mode = mode;
        paths.push(input);
    }
    for hand in Hand::BOTH {
        let mut occupied = observation(old, Some(FOLDED));
        occupied.occupied[hand.index()] = true;
        paths.push(occupied);
        let mut claim = observation(old, Some(FOLDED));
        claim.claims[hand.index()].mechanical = true;
        paths.push(claim);
    }
    for rejected in paths {
        let mut state = WingsuitGestures::default();
        state.step(observation(high, Some(FOLDED)), &CONFIG);
        assert_eq!(state.step(rejected, &CONFIG), GestureCommands::default());
        assert_eq!(state.last, Some((high, SUIT)));
        let disabled = WingsuitGesturesConfig {
            enabled: false,
            ..CONFIG
        };
        assert_eq!(state.step(rejected, &disabled), GestureCommands::default());
        assert_eq!(state.config, Some(CONFIG));
        assert_eq!(state.last, Some((high, SUIT)));
        assert_eq!(
            state.step(observation(high, Some(FOLDED)), &CONFIG),
            GestureCommands::default()
        );
        assert!(!state.ready && state.neutral.since_ns.is_none());
        let next = GestureSample {
            sequence: 101,
            time_ns: 1_050_000_000,
            ..high
        };
        state.step(observation(next, Some(FOLDED)), &CONFIG);
        assert_eq!(state.neutral.since_ns, Some(next.time_ns));
        assert!(!state.ready);
    }
}

#[test]
fn time_sequence_and_epoch_rollback_are_rejected_before_mode_or_config_updates() {
    let high = GestureSample {
        epoch: 3,
        sequence: 100,
        time_ns: 1_000_000_000,
    };
    for bad in [
        GestureSample {
            sequence: 100,
            time_ns: 1_050_000_000,
            ..high
        },
        GestureSample {
            sequence: 99,
            time_ns: 1_050_000_000,
            ..high
        },
        GestureSample {
            sequence: 101,
            time_ns: 1_000_000_000,
            ..high
        },
        GestureSample {
            sequence: 101,
            time_ns: 950_000_000,
            ..high
        },
        GestureSample {
            epoch: 2,
            sequence: 200,
            time_ns: 2_000_000_000,
        },
    ] {
        let mut state = WingsuitGestures::default();
        state.step(observation(high, Some(FOLDED)), &CONFIG);
        let mut input = observation(bad, Some(FLIGHT));
        input.mode = FALLING;
        assert_eq!(
            state.step(
                input,
                &WingsuitGesturesConfig {
                    enabled: false,
                    ..CONFIG
                }
            ),
            GestureCommands::default()
        );
        assert_eq!(state.last, Some((high, SUIT)));
        assert_eq!(state.config, Some(CONFIG));
        assert_eq!(
            state.step(observation(high, Some(FOLDED)), &CONFIG),
            GestureCommands::default()
        );
        assert!(!state.ready);
        state.reset();
        assert_eq!(state.last, Some((high, SUIT)));
        let new_epoch = GestureSample {
            epoch: 4,
            sequence: 1,
            time_ns: 0,
        };
        state.step(observation(new_epoch, Some(FOLDED)), &CONFIG);
        assert_eq!(state.last, Some((new_epoch, SUIT)));
        assert_eq!(state.neutral.since_ns, Some(0));
        assert_eq!(
            state.step(observation(high, Some(FOLDED)), &CONFIG),
            GestureCommands::default()
        );
        assert_eq!(state.last, Some((new_epoch, SUIT)));
    }
}

#[test]
fn repeated_samples_retain_only_held_commands_and_cancellation_does_not_rearm_them() {
    let mut h = History::new(SUIT);
    h.arm();
    h.hold(BRAKE, 6);
    let same = observation(h.sample, Some(BRAKE));
    for _ in 0..20 {
        let command = h.state.step(same, &CONFIG);
        assert!(command.brake && command.reserve_pitch && command.transition.is_none());
    }
    for cancel in [
        GestureInput {
            gameplay: false,
            ..same
        },
        GestureInput {
            canopy_requested: true,
            ..same
        },
        GestureInput {
            mode: CANOPY,
            ..same
        },
        GestureInput { pose: None, ..same },
    ] {
        assert_eq!(h.state.step(cancel, &CONFIG), GestureCommands::default());
        assert_eq!(h.state.step(same, &CONFIG), GestureCommands::default());
        assert!(!h.state.armed);
    }
}

#[test]
fn falling_and_suit_boundaries_require_a_new_neutral_to_flight_sequence() {
    for (start, next) in [(FALLING, SUIT), (SUIT, FALLING)] {
        let mut h = History::new(start);
        h.hold(FOLDED, 5);
        h.hold(FLIGHT, 6);
        h.mode = next;
        assert!(
            h.hold(FLIGHT, 20)
                .iter()
                .all(|command| *command == GestureCommands::default())
        );
        assert!(!h.state.armed && !h.state.ready);
        h.hold(FOLDED, 5);
        let commands = h.hold(FLIGHT, 6);
        if next == SUIT {
            assert!(h.state.armed);
        } else {
            assert_eq!(
                commands
                    .iter()
                    .filter(|command| command.transition == Some(SuitTransition::Deploy))
                    .count(),
                1
            );
        }
    }
}

struct TrackingFixture {
    sample: GestureSample,
    head: GestureHead,
    frame: ControllerFrame,
    offset: GripOffset,
    dimensions: BodyDimensions,
}

impl TrackingFixture {
    fn new() -> Self {
        let published_at = Instant::now();
        let sample = GestureSample {
            epoch: 7,
            sequence: 10,
            time_ns: 400_000_000,
        };
        let tracked = |pose| TrackedPose {
            pose,
            position_valid: true,
            orientation_valid: true,
            position_tracked: true,
            orientation_tracked: true,
            ..Default::default()
        };
        let dimensions = BodyDimensions::GENERIC_ADULT;
        let head_pose = Pose {
            position: Vec3::new(0.0, dimensions.eye_height, 0.0),
            orientation: Quat::IDENTITY,
        };
        let shoulders = body::shoulder_positions(head_pose.position, Quat::IDENTITY, &dimensions);
        let hands = Hand::BOTH.map(|hand| {
            let side = if hand == Hand::Left { -1.0 } else { 1.0 };
            HandFrame {
                active: true,
                grip: tracked(Pose {
                    position: shoulders[hand.index()] + Vec3::X * side * 0.60,
                    orientation: Quat::from_rotation_y(-side * FRAC_PI_2),
                }),
                ..Default::default()
            }
        });
        Self {
            sample,
            head: GestureHead {
                sample,
                pose: tracked(head_pose),
                published_at,
            },
            frame: ControllerFrame {
                sequence: sample.sequence,
                display_time_ns: sample.time_ns,
                published_at,
                focused: true,
                action_set: ActionSetKind::Wingsuit,
                hands,
            },
            offset: GripOffset {
                position_m: Vec3::ZERO,
                rotation_deg: Vec3::ZERO,
            },
            dimensions,
        }
    }

    fn measure(&self) -> Option<FlightPose> {
        FlightPose::from_tracking(&GestureObservation {
            sample: self.sample,
            head: self.head,
            controllers: &self.frame,
            grip_offset: &self.offset,
            dimensions: &self.dimensions,
            torso: None,
            now: self.frame.published_at,
        })
    }
}

#[test]
fn explicit_pose_adapter_rejects_mixed_untracked_and_stale_head_observations() {
    assert!(TrackingFixture::new().measure().is_some());
    for change in 0..9 {
        let mut fixture = TrackingFixture::new();
        match change {
            0 => fixture.head.sample.sequence -= 1,
            1 => fixture.head.sample.time_ns -= 1,
            2 => fixture.head.sample.epoch -= 1,
            3 => fixture.head.pose.position_tracked = false,
            4 => fixture.head.pose.orientation_tracked = false,
            5 => fixture.head.published_at -= Duration::from_nanos(1),
            6 => fixture.frame.focused = false,
            7 => fixture.frame.hands[0].grip.position_tracked = false,
            _ => fixture.head.pose.pose.position.x = f32::NAN,
        }
        assert!(fixture.measure().is_none(), "case {change}");
    }
    let fixture = TrackingFixture::new();
    assert!(
        FlightPose::from_tracking(&GestureObservation {
            sample: fixture.sample,
            head: fixture.head,
            controllers: &fixture.frame,
            grip_offset: &fixture.offset,
            dimensions: &fixture.dimensions,
            torso: None,
            now: fixture.frame.published_at + Duration::from_secs(10),
        })
        .is_none()
    );
}

#[test]
fn grip_palm_basis_has_independent_expected_normals_for_both_hands_and_adjustments() {
    let mut fixture = TrackingFixture::new();
    let forward = fixture.measure().unwrap();
    assert!(forward.palm_forward.into_iter().all(|value| value > 0.999));
    let mut changed_aim = fixture.frame.clone();
    for hand in &mut changed_aim.hands {
        hand.aim.pose.orientation = Quat::from_rotation_z(1.2);
        hand.aim.pose.position = Vec3::splat(999.0);
    }
    fixture.frame = changed_aim;
    assert_eq!(fixture.measure(), Some(forward));
    fixture.offset.rotation_deg = Vec3::new(0.0, 30.0, 0.0);
    let tilted = fixture.measure().unwrap();
    assert!(
        tilted
            .palm_forward
            .into_iter()
            .all(|value| (value - 30.0_f32.to_radians().cos()).abs() < 1e-5)
    );
    fixture.offset.rotation_deg = Vec3::new(0.0, 90.0, 0.0);
    assert!(
        fixture
            .measure()
            .unwrap()
            .palm_forward
            .into_iter()
            .all(|value| value.abs() < 1e-5)
    );
    fixture.offset = GripOffset::INDEX_BEST_GUESS;
    assert!(
        fixture
            .measure()
            .unwrap()
            .palm_forward
            .into_iter()
            .all(|value| value > 0.999)
    );
    for hand in &mut fixture.frame.hands {
        hand.grip.pose.orientation = Quat::IDENTITY;
    }
    assert!(
        fixture
            .measure()
            .unwrap()
            .palm_forward
            .into_iter()
            .all(|value| value.abs() < 1e-5)
    );
}

#[test]
fn config_endpoints_and_pose_hysteresis_boundaries_remain_separated() {
    assert!(
        WingsuitGesturesConfig {
            flight_lateral_span: 0.15,
            flight_angle_deg: 15.0,
            withdraw_lateral_span: 0.0,
            withdraw_lowered_span: 0.10,
            withdraw_reach_span: 0.10,
            pose_hysteresis_span: 0.01,
            brake_angle_deg: 15.0,
            brake_hysteresis_deg: 2.0,
            pitch_release_alignment: 0.0,
            neutral_dwell_s: 0.05,
            flight_dwell_s: 0.10,
            close_dwell_s: 0.15,
            brake_dwell_s: 0.05,
            max_sample_gap_s: 0.05,
            ..CONFIG
        }
        .valid()
    );
    assert!(
        WingsuitGesturesConfig {
            flight_lateral_span: 0.40,
            flight_angle_deg: 60.0,
            withdraw_lateral_span: 0.18,
            withdraw_lowered_span: 0.35,
            withdraw_reach_span: 0.35,
            pose_hysteresis_span: 0.08,
            brake_angle_deg: 60.0,
            brake_hysteresis_deg: 20.0,
            pitch_release_alignment: 0.0,
            neutral_dwell_s: 0.50,
            flight_dwell_s: 1.0,
            close_dwell_s: 1.0,
            brake_dwell_s: 0.50,
            max_sample_gap_s: 0.50,
            ..CONFIG
        }
        .valid()
    );
    let boundary_down = measured([Vec3::new(0.0, -0.18 * 1.8, 0.0); 2]);
    assert!(boundary_down.withdrawn(&CONFIG));
    let shallow = measured([Vec3::new(0.0, -0.17 * 1.8, -0.30 * 1.8); 2]);
    assert!(!shallow.withdrawn(&CONFIG));
    for invalid in [
        WingsuitGesturesConfig {
            withdraw_lowered_span: f32::NAN,
            ..CONFIG
        },
        WingsuitGesturesConfig {
            withdraw_lowered_span: 0.0,
            ..CONFIG
        },
        WingsuitGesturesConfig {
            brake_angle_deg: 60.0,
            brake_hysteresis_deg: 20.0,
            pitch_release_alignment: 0.40,
            ..CONFIG
        },
        WingsuitGesturesConfig {
            flight_angle_deg: 61.0,
            ..CONFIG
        },
        WingsuitGesturesConfig {
            pose_hysteresis_span: 0.0,
            ..CONFIG
        },
        WingsuitGesturesConfig {
            withdraw_reach_span: 0.351,
            ..CONFIG
        },
        WingsuitGesturesConfig {
            withdraw_lateral_span: 0.18,
            flight_lateral_span: 0.20,
            ..CONFIG
        },
    ] {
        assert!(!invalid.valid());
    }
    let mut h = History::new(SUIT);
    h.arm();
    let below_entry = FlightPose {
        palm_forward: [(CONFIG.brake_angle_deg + 0.01).to_radians().cos(); 2],
        ..FLIGHT
    };
    assert_eq!(h.observe(Some(below_entry)), GestureCommands::default());
    assert!(h.hold(BRAKE, 6).iter().any(|command| command.brake));
    let retained = FlightPose {
        lateral: [CONFIG.flight_lateral_span - CONFIG.pose_hysteresis_span; 2],
        palm_forward: [(CONFIG.brake_angle_deg + CONFIG.brake_hysteresis_deg)
            .to_radians()
            .cos(); 2],
        ..FLIGHT
    };
    assert!(h.observe(Some(retained)).brake);
    let released = FlightPose {
        lateral: [retained.lateral[0] - 0.001; 2],
        ..retained
    };
    let output = h.observe(Some(released));
    assert!(!output.brake && output.reserve_pitch);
    assert!(!h.observe(Some(FLIGHT)).reserve_pitch);
    assert!(h.hold(BRAKE, 6).iter().any(|command| command.brake));
    let beyond_angle = FlightPose {
        palm_forward: [(CONFIG.brake_angle_deg + CONFIG.brake_hysteresis_deg + 0.01)
            .to_radians()
            .cos(); 2],
        ..FLIGHT
    };
    let output = h.observe(Some(beyond_angle));
    assert!(!output.brake && output.reserve_pitch);
    let pitch = Some(Vec2::new(-0.8, 0.9));
    assert_eq!(reserve_brake_pitch(pitch, false), pitch);
    assert_eq!(reserve_brake_pitch(None, true), None);
    assert_eq!(reserve_brake_pitch(pitch, true), Some(Vec2::new(-0.8, 0.0)));
    assert_eq!(
        combine(Vec2::new(-0.5, -0.7), reserve_brake_pitch(pitch, true)),
        Vec2::new(-1.0, -0.7)
    );
    assert_eq!(
        combine(Vec2::new(0.2, -0.3), reserve_brake_pitch(None, true)),
        Vec2::new(0.2, -0.3)
    );
}
