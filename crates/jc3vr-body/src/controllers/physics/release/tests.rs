use std::time::{Duration, Instant};

use glam::Quat;
use jc3vr_input::{
    frame::{ActionSetKind, ControllerFrame, HandFrame, TrackedPose},
    motion::MotionSample,
};
use tracking_format::event::WorldMotion;

use super::*;

fn controller(hand: Hand) -> ControllerContext {
    let mut hands: [HandFrame; 2] = Default::default();
    hands[hand.index()].active = true;
    hands[hand.index()].grip = TrackedPose {
        position_valid: true,
        orientation_valid: true,
        position_tracked: true,
        orientation_tracked: true,
        ..TrackedPose::default()
    };
    ControllerContext::from_frame(
        &ControllerFrame {
            sequence: 1,
            display_time_ns: 1_000_000_000,
            published_at: Instant::now(),
            focused: true,
            action_set: ActionSetKind::OnFoot,
            hands,
        },
        hand,
    )
}

fn release_frame() -> ReleaseFrame {
    ReleaseFrame {
        origin: Vec3::new(100.0, 20.0, -30.0),
        rig: RigSample {
            rotation: Quat::IDENTITY,
            rebases: 0,
            turns: 0,
        },
        scale: 1.0,
        teleport_epoch: 0,
    }
}

fn release_sample(time_s: f64, velocity: Vec3, angular: Vec3) -> MotionSample {
    MotionSample {
        time_s,
        position: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        linear_velocity: Some(velocity),
        angular_velocity: Some(angular),
    }
}

fn assert_motion(actual: WorldMotion, linear: Vec3, angular: Vec3) {
    assert!(actual.linear.distance(linear) < 1e-3, "{actual:?}");
    assert!(actual.angular.distance(angular) < 1e-3, "{actual:?}");
}

#[test]
fn a_release_takes_the_hands_fastest_moment_with_the_rigs_motion_for_either_hand() {
    for hand in Hand::BOTH {
        let context = controller(hand);
        let rig = RigMotion {
            frame: release_frame(),
            motion: WorldMotion {
                linear: Vec3::X * 3.0,
                angular: Vec3::ZERO,
            },
        };
        let samples = [
            release_sample(0.95, Vec3::NEG_Z * 8.0, Vec3::Y * 4.0),
            release_sample(1.0, Vec3::NEG_Z, Vec3::Y),
            release_sample(1.01, Vec3::splat(100.0), Vec3::splat(100.0)),
        ];
        let release = estimate(ReleaseInput {
            rig: Some(rig),
            controller: Some(&context),
            samples: &samples,
            since_s: 0.9,
            history_after_s: None,
            point_world: rig.frame.origin,
        });
        assert_motion(
            release.capped(20.0),
            Vec3::new(3.0, 0.0, -8.0),
            Vec3::Y * 4.0,
        );
        assert_eq!(release.source, ReleaseSource::TimestampedHandHistory);
        assert_eq!(release.history_age_s, Some(0.0));
    }
}

#[test]
fn release_converts_point_motion_and_scale_once_and_rig_motion_once() {
    let context = controller(Hand::Left);
    let rig = RigMotion {
        frame: ReleaseFrame {
            rig: RigSample {
                rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                ..release_frame().rig
            },
            scale: 2.0,
            ..release_frame()
        },
        motion: WorldMotion {
            linear: Vec3::X * 5.0,
            angular: Vec3::Y,
        },
    };
    let point = rig.frame.origin + rig.frame.rig.rotation * (Vec3::NEG_Z * 0.2);
    let sample = release_sample(1.0, Vec3::X * 2.0, Vec3::Y * 3.0);
    let release = estimate(ReleaseInput {
        rig: Some(rig),
        controller: Some(&context),
        samples: &[sample],
        since_s: 0.9,
        history_after_s: None,
        point_world: point,
    });
    let hand_linear = rig.frame.rig.rotation * (Vec3::X * 1.7) * 2.0;
    let transport = rig.motion.linear + rig.motion.angular.cross(point - rig.frame.origin);
    assert_motion(release.world, hand_linear + transport, Vec3::Y * 4.0);
    let requested = release.capped(2.0);
    // The hand's own part is held to the cap along its own direction; what the player's movement
    // carries is kept whole.
    let carried = release.inherited.linear;
    let own = requested.linear - carried;
    assert!((own.length() - 2.0).abs() < 1e-3, "{own}");
    assert!(
        own.normalize()
            .distance((release.world.linear - carried).normalize())
            < 1e-3
    );
    assert_eq!(requested.angular, release.world.angular);
}

#[test]
fn missing_or_stale_tracking_releases_with_transport_not_an_old_throw_peak() {
    let context = controller(Hand::Right);
    let rig = RigMotion {
        frame: release_frame(),
        motion: WorldMotion {
            linear: Vec3::X * 6.0,
            angular: Vec3::Y,
        },
    };
    let point = rig.frame.origin + Vec3::Z;
    let samples = [release_sample(1.0, Vec3::Z * 40.0, Vec3::X * 40.0)];
    for invalid in [
        ControllerContext {
            focused: false,
            ..context
        },
        ControllerContext {
            position_tracked: false,
            ..context
        },
        ControllerContext {
            orientation_tracked: false,
            ..context
        },
        ControllerContext {
            publication_age: Duration::from_secs(1),
            ..context
        },
        ControllerContext {
            display_time_ns: 2_000_000_000,
            ..context
        },
    ] {
        let release = estimate(ReleaseInput {
            rig: Some(rig),
            controller: Some(&invalid),
            samples: &samples,
            since_s: 0.0,
            history_after_s: None,
            point_world: point,
        });
        assert_eq!(release.source, ReleaseSource::RigTransportOnly);
        assert_motion(release.world, Vec3::X * 7.0, Vec3::Y);
    }
}

#[test]
fn acquisition_bound_excludes_the_previous_items_throw_and_slow_release_stays_slow() {
    let context = controller(Hand::Right);
    let rig = RigMotion {
        frame: release_frame(),
        motion: WorldMotion {
            linear: Vec3::ZERO,
            angular: Vec3::ZERO,
        },
    };
    let samples = [
        release_sample(0.9, Vec3::Z * 20.0, Vec3::Y * 12.0),
        release_sample(1.0, Vec3::X * 0.1, Vec3::ZERO),
    ];
    let release = estimate(ReleaseInput {
        rig: Some(rig),
        controller: Some(&context),
        samples: &samples,
        since_s: 0.99,
        history_after_s: None,
        point_world: rig.frame.origin,
    });
    assert_motion(release.world, Vec3::X * 0.1, Vec3::ZERO);
}

#[test]
fn release_uses_position_and_orientation_differences_when_runtime_omits_velocities() {
    let context = controller(Hand::Right);
    let rig = RigMotion {
        frame: release_frame(),
        motion: WorldMotion {
            linear: Vec3::ZERO,
            angular: Vec3::ZERO,
        },
    };
    let samples = [
        MotionSample {
            position: Vec3::ZERO,
            linear_velocity: None,
            angular_velocity: None,
            ..release_sample(0.98, Vec3::ZERO, Vec3::ZERO)
        },
        MotionSample {
            position: Vec3::X * 0.04,
            orientation: -Quat::from_rotation_y(0.06),
            linear_velocity: None,
            angular_velocity: None,
            ..release_sample(1.0, Vec3::ZERO, Vec3::ZERO)
        },
    ];
    let release = estimate(ReleaseInput {
        rig: Some(rig),
        controller: Some(&context),
        samples: &samples,
        since_s: 0.9,
        history_after_s: None,
        point_world: rig.frame.origin + Vec3::X * 0.04,
    });
    assert_motion(release.world, Vec3::X * 2.0, Vec3::Y * 3.0);
}

#[test]
fn a_missing_runtime_angular_velocity_still_moves_the_released_point() {
    let context = controller(Hand::Left);
    let rig = RigMotion {
        frame: release_frame(),
        motion: WorldMotion {
            linear: Vec3::ZERO,
            angular: Vec3::ZERO,
        },
    };
    let samples = [
        MotionSample {
            angular_velocity: None,
            ..release_sample(0.95, Vec3::ZERO, Vec3::ZERO)
        },
        MotionSample {
            orientation: Quat::from_rotation_y(0.1),
            angular_velocity: None,
            ..release_sample(1.0, Vec3::ZERO, Vec3::ZERO)
        },
    ];
    let lever = samples[1].orientation * (Vec3::NEG_Z * 0.2);
    let release = estimate(ReleaseInput {
        rig: Some(rig),
        controller: Some(&context),
        samples: &samples,
        since_s: 0.9,
        history_after_s: None,
        point_world: rig.frame.origin + lever,
    });
    assert_motion(release.world, (Vec3::Y * 2.0).cross(lever), Vec3::Y * 2.0);
}

#[test]
fn transport_is_finite_differenced_once_and_discontinuities_reprime_it() {
    let mut transport = ReleaseTransport::new();
    let first = release_frame();
    assert_eq!(transport.update(Some(first), 0.02, Some(0.9)), None);
    let next = ReleaseFrame {
        origin: first.origin + Vec3::X * 0.2,
        ..first
    };
    let rig = transport.update(Some(next), 0.02, Some(0.92)).unwrap();
    assert_motion(rig.motion, Vec3::X * 10.0, Vec3::ZERO);
    for reset in [
        ReleaseFrame {
            teleport_epoch: 1,
            ..next
        },
        ReleaseFrame {
            rig: RigSample {
                rebases: 1,
                ..next.rig
            },
            ..next
        },
        ReleaseFrame { scale: 2.0, ..next },
        ReleaseFrame {
            rig: RigSample {
                rotation: Quat::from_rotation_y(PHYSICS.get().rig.unannounced_turn_rad + 0.1),
                ..next.rig
            },
            ..next
        },
        ReleaseFrame {
            origin: Vec3::splat(f32::NAN),
            ..next
        },
    ] {
        let mut transport = ReleaseTransport::new();
        transport.update(Some(next), 0.02, Some(0.9));
        assert_eq!(transport.update(Some(reset), 0.02, Some(0.94)), None);
        assert_eq!(transport.history_after_s(), Some(0.94));
    }
    assert_eq!(transport.update(Some(next), 1.0, Some(1.94)), None);
    assert_eq!(transport.update(None, 0.02, Some(1.96)), None);
    assert_eq!(transport.update(Some(next), 0.02, Some(1.98)), None);
}

#[test]
fn composed_head_shift_does_not_become_a_wrist_rotation_lever() {
    let context = controller(Hand::Right);
    let base = ReleaseFrame {
        rig: RigSample {
            rotation: Quat::from_rotation_y(0.4),
            ..release_frame().rig
        },
        scale: 1.5,
        ..release_frame()
    };
    let origin = base.origin + Vec3::new(0.5, 0.2, -0.1);
    let head_cockpit = Vec3::new(0.25, 0.1, -0.3);
    let head_world = origin + base.rig.rotation * (head_cockpit * base.scale);
    let frame = base.with_composed_head(head_world, head_cockpit);
    assert!(frame.origin.distance(origin) < 1e-4);
    let sample = MotionSample {
        position: Vec3::new(0.3, -0.1, -0.4),
        orientation: Quat::from_rotation_y(0.35),
        ..release_sample(1.0, Vec3::ZERO, Vec3::Y * 4.0)
    };
    let lever = sample.orientation * (Vec3::NEG_Z * 0.2);
    let point = origin + frame.rig.rotation * ((sample.position + lever) * frame.scale);
    let release = estimate(ReleaseInput {
        rig: Some(RigMotion {
            frame,
            motion: WorldMotion {
                linear: Vec3::ZERO,
                angular: Vec3::ZERO,
            },
        }),
        controller: Some(&context),
        samples: &[sample],
        since_s: 0.9,
        history_after_s: None,
        point_world: point,
    });
    assert_eq!(release.source, ReleaseSource::TimestampedHandHistory);
    assert_motion(
        release.world,
        frame.rig.rotation * (Vec3::Y * 4.0).cross(lever) * frame.scale,
        Vec3::Y * 4.0,
    );
}

#[test]
fn changing_composed_origin_transports_stationary_tracking_once() {
    let base = release_frame();
    let origin = base.origin + Vec3::new(0.5, 0.2, -0.1);
    let raw_head = Vec3::new(0.2, 0.1, -0.3);
    let previous = base.with_composed_head(origin + raw_head, raw_head);
    let current_origin = origin + Vec3::X * 0.02;
    let current_head = raw_head + Vec3::X * 0.1;
    let current = ReleaseFrame {
        rig: RigSample {
            rotation: Quat::from_rotation_y(0.08),
            ..base.rig
        },
        ..base
    };
    let current = current.with_composed_head(
        current_origin + current.rig.rotation * current_head,
        current_head,
    );
    let mut transport = ReleaseTransport::new();
    assert_eq!(transport.update(Some(previous), 0.02, Some(0.98)), None);
    let rig = transport.update(Some(current), 0.02, Some(1.0)).unwrap();
    assert_motion(rig.motion, Vec3::X, Vec3::Y * 4.0);
    let sample = MotionSample {
        position: Vec3::new(0.3, -0.1, -0.4),
        ..release_sample(1.0, Vec3::ZERO, Vec3::ZERO)
    };
    let lever = current.rig.rotation * sample.position;
    let context = controller(Hand::Left);
    let release = estimate(ReleaseInput {
        rig: Some(rig),
        controller: Some(&context),
        samples: &[sample],
        since_s: 0.9,
        history_after_s: transport.history_after_s(),
        point_world: current_origin + lever,
    });
    assert_eq!(release.source, ReleaseSource::TimestampedHandHistory);
    assert_motion(
        release.world,
        Vec3::X + (Vec3::Y * 4.0).cross(lever),
        Vec3::Y * 4.0,
    );
}

#[test]
fn second_input_tick_after_recenter_excludes_old_peak_and_coordinate_difference() {
    for runtime_velocities in [false, true] {
        let mut transport = ReleaseTransport::new();
        let previous = release_frame();
        assert_eq!(transport.update(Some(previous), 0.02, Some(0.94)), None);
        assert!(transport.update(Some(previous), 0.02, Some(0.96)).is_some());
        let recentered = ReleaseFrame {
            rig: RigSample {
                rebases: 1,
                ..previous.rig
            },
            ..previous
        };
        assert_eq!(transport.update(Some(recentered), 0.02, Some(0.98)), None);
        assert_eq!(transport.history_after_s(), Some(0.98));
        let rig = transport.update(Some(recentered), 0.02, Some(1.0)).unwrap();
        assert_eq!(transport.history_after_s(), Some(0.98));
        let sample = |time_s, position, orientation, linear, angular| MotionSample {
            position,
            orientation,
            linear_velocity: runtime_velocities.then_some(linear),
            angular_velocity: runtime_velocities.then_some(angular),
            ..release_sample(time_s, Vec3::ZERO, Vec3::ZERO)
        };
        let samples = [
            sample(
                0.96,
                Vec3::ZERO,
                Quat::IDENTITY,
                Vec3::Z * 20.0,
                Vec3::Y * 12.0,
            ),
            sample(
                0.98,
                Vec3::ZERO,
                Quat::IDENTITY,
                Vec3::Z * 20.0,
                Vec3::Y * 12.0,
            ),
            sample(
                0.99,
                Vec3::X * 0.5,
                Quat::from_rotation_y(0.5),
                Vec3::ZERO,
                Vec3::ZERO,
            ),
            sample(
                1.0,
                Vec3::X * 0.5,
                Quat::from_rotation_y(0.5),
                Vec3::ZERO,
                Vec3::ZERO,
            ),
        ];
        let context = controller(Hand::Right);
        let release = estimate(ReleaseInput {
            rig: Some(rig),
            controller: Some(&context),
            samples: &samples,
            since_s: 0.8,
            history_after_s: transport.history_after_s(),
            point_world: rig.frame.origin + Vec3::X * 0.5,
        });
        assert_eq!(release.source, ReleaseSource::TimestampedHandHistory);
        assert_motion(release.world, Vec3::ZERO, Vec3::ZERO);
        assert_eq!(release.history_age_s, Some(0.0));

        let acquired_later = estimate(ReleaseInput {
            rig: Some(rig),
            controller: Some(&context),
            samples: &[
                release_sample(0.99, Vec3::Z * 20.0, Vec3::Y * 12.0),
                release_sample(1.0, Vec3::X * 0.1, Vec3::ZERO),
            ],
            since_s: 0.995,
            history_after_s: transport.history_after_s(),
            point_world: rig.frame.origin,
        });
        assert_motion(acquired_later.world, Vec3::X * 0.1, Vec3::ZERO);

        let no_current_samples = estimate(ReleaseInput {
            rig: Some(rig),
            controller: Some(&context),
            samples: &samples[..2],
            since_s: 0.8,
            history_after_s: transport.history_after_s(),
            point_world: rig.frame.origin,
        });
        assert_eq!(no_current_samples.source, ReleaseSource::RigTransportOnly);
        assert_motion(no_current_samples.world, Vec3::ZERO, Vec3::ZERO);
    }
}

#[test]
fn missing_coordinate_boundary_time_waits_for_a_dated_sample() {
    let mut transport = ReleaseTransport::new();
    let frame = release_frame();
    assert_eq!(transport.update(Some(frame), 0.02, None), None);
    assert_eq!(transport.history_after_s(), Some(f64::INFINITY));
    assert!(transport.update(Some(frame), 0.02, None).is_some());
    assert_eq!(transport.history_after_s(), Some(f64::INFINITY));
    assert!(transport.update(Some(frame), 0.02, Some(1.0)).is_some());
    assert_eq!(transport.history_after_s(), Some(1.0));
    assert!(transport.update(Some(frame), 0.02, Some(1.02)).is_some());
    assert_eq!(transport.history_after_s(), Some(1.0));
}

#[test]
fn an_unprimed_rig_or_bad_request_never_reuses_cached_motion() {
    let context = controller(Hand::Left);
    let release = estimate(ReleaseInput {
        rig: None,
        controller: Some(&context),
        samples: &[release_sample(1.0, Vec3::X * 40.0, Vec3::Y * 20.0)],
        since_s: 0.0,
        history_after_s: None,
        point_world: Vec3::ZERO,
    });
    assert_eq!(release, ReleaseMotion::REST);
    for cap in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let requested = ReleaseMotion {
            world: WorldMotion {
                linear: Vec3::X * 4.0,
                angular: Vec3::splat(f32::NAN),
            },
            ..ReleaseMotion::REST
        }
        .capped(cap);
        assert_motion(requested, Vec3::ZERO, Vec3::ZERO);
    }
}

#[test]
fn a_slowed_world_takes_the_hands_part_of_a_throw_at_the_hands_real_speed() {
    let slow = TimeScale::new(0.25);
    let thrown = ReleaseMotion {
        world: WorldMotion {
            linear: Vec3::new(6.0, 2.0, 0.0),
            angular: Vec3::Y * 3.0,
        },
        inherited: WorldMotion {
            linear: Vec3::X * 2.0,
            angular: Vec3::Y,
        },
        source: ReleaseSource::TimestampedHandHistory,
        history_age_s: Some(0.01),
        time_scale: TimeScale::REAL,
    };
    let game = thrown.in_game_time(slow);
    // The hand's 4 m/s along X and 2 m/s up become four times as fast per game second; the
    // player's own movement, already per game second, is kept.
    assert_eq!(game.world.linear, Vec3::new(2.0 + 16.0, 8.0, 0.0));
    assert_eq!(game.world.angular, Vec3::Y * (1.0 + 8.0));
    assert_eq!(game.inherited, thrown.inherited);
    assert_eq!(thrown.in_game_time(TimeScale::REAL), thrown);
    // The cap holds the hand's part of the throw to the same real speed at any time scale, and
    // leaves the player's carried movement whole.
    let carried = thrown.inherited.linear;
    let capped = game.capped(2.0);
    assert!(
        ((capped.linear - carried).length() - 8.0).abs() < 1e-4,
        "{capped:?}"
    );
    assert!(((thrown.capped(2.0).linear - carried).length() - 2.0).abs() < 1e-4);
}

#[test]
fn the_carried_share_passes_the_hands_cap_bounded_only_by_the_sanity_limit() {
    let carried = |linear: Vec3| ReleaseMotion {
        world: WorldMotion {
            linear: linear + Vec3::Y * 30.0,
            angular: Vec3::ZERO,
        },
        inherited: WorldMotion {
            linear,
            angular: Vec3::ZERO,
        },
        source: ReleaseSource::TimestampedHandHistory,
        history_age_s: Some(0.0),
        time_scale: TimeScale::REAL,
    };
    let max_hand = PHYSICS.get().release.max_hand_mps;
    let max_carried = PHYSICS.get().release.max_carried_mps;
    // A car at 40 m/s keeps its speed; only the hand's 30 m/s is held to the cap.
    let thrown = carried(Vec3::X * 40.0).thrown();
    assert_motion(thrown, Vec3::new(40.0, max_hand, 0.0), Vec3::ZERO);
    // A reading past any real speed is cut to the sanity limit; what it was over counts as the
    // hand's share, which the hand's cap holds.
    let corrupt = carried(Vec3::X * (max_carried * 10.0)).thrown().linear;
    assert!(
        corrupt.length() <= max_carried + max_hand + 1e-3,
        "{corrupt}"
    );
    assert!(corrupt.x >= max_carried, "{corrupt}");
    // A cap of zero drops the object with the player's movement alone.
    assert_motion(
        carried(Vec3::X * 40.0).capped(0.0),
        Vec3::X * 40.0,
        Vec3::ZERO,
    );
}

#[test]
fn a_heavy_objects_centre_of_mass_leaves_with_the_hands_lever_and_spin() {
    let context = controller(Hand::Right);
    let rig = RigMotion {
        frame: release_frame(),
        motion: WorldMotion::ZERO,
    };
    // The grip moves forward and turns about up; the centre of mass sits half a metre to its right.
    let sample = release_sample(1.0, Vec3::NEG_Z * 2.0, Vec3::Y * 4.0);
    let centre = Vec3::X * 0.5;
    let release = estimate(ReleaseInput {
        rig: Some(rig),
        controller: Some(&context),
        samples: &[sample],
        since_s: 0.9,
        history_after_s: None,
        point_world: rig.frame.origin + centre,
    });
    let lever = (Vec3::Y * 4.0).cross(centre);
    assert_motion(release.thrown(), Vec3::NEG_Z * 2.0 + lever, Vec3::Y * 4.0);
}

#[test]
fn a_snap_turn_leaves_the_rig_at_rest_for_its_tick_and_keeps_the_hands_history() {
    let mut transport = ReleaseTransport::new();
    let first = release_frame();
    transport.update(Some(first), 0.02, Some(0.9));
    assert!(transport.update(Some(first), 0.02, Some(0.92)).is_some());
    let history = transport.history_after_s();
    // A thirty-degree snap: the body frame and the cockpit's origin about the head jump in a tick.
    let snapped = ReleaseFrame {
        origin: first.origin + Vec3::X * 0.3,
        rig: RigSample {
            rotation: Quat::from_rotation_y(30f32.to_radians()),
            turns: 1,
            ..first.rig
        },
        ..first
    };
    let rig = transport.update(Some(snapped), 0.02, Some(0.94)).unwrap();
    assert_eq!(rig.motion, WorldMotion::ZERO);
    assert_eq!(transport.history_after_s(), history);
    // So a throw on that tick takes the hand's own motion and no spin from the turn.
    let context = controller(Hand::Right);
    let release = estimate(ReleaseInput {
        rig: Some(rig),
        controller: Some(&context),
        samples: &[release_sample(1.0, Vec3::NEG_Z * 5.0, Vec3::ZERO)],
        since_s: 0.9,
        history_after_s: transport.history_after_s(),
        point_world: rig.frame.origin + Vec3::NEG_Z * 0.5,
    });
    assert_motion(
        release.thrown(),
        rig.frame.rig.rotation * (Vec3::NEG_Z * 5.0),
        Vec3::ZERO,
    );
    // The tick after it is part of the same jump, then the rig moves again.
    let still = transport.update(Some(snapped), 0.02, Some(0.96)).unwrap();
    assert_eq!(still.motion, WorldMotion::ZERO);
    let moved = ReleaseFrame {
        origin: snapped.origin + Vec3::X * 0.02,
        ..snapped
    };
    let rig = transport.update(Some(moved), 0.02, Some(0.98)).unwrap();
    assert_motion(rig.motion, Vec3::X, Vec3::ZERO);
}

#[test]
fn a_fast_smooth_turn_is_the_rigs_motion_and_a_game_turn_past_the_fallback_cuts() {
    let fallback = PHYSICS.get().rig.unannounced_turn_rad;
    let mut transport = ReleaseTransport::new();
    let first = release_frame();
    transport.update(Some(first), 0.02, Some(0.9));
    let turned = ReleaseFrame {
        rig: RigSample {
            rotation: Quat::from_rotation_y(fallback * 0.9),
            ..first.rig
        },
        ..first
    };
    let rig = transport.update(Some(turned), 0.02, Some(0.92)).unwrap();
    assert_motion(rig.motion, Vec3::ZERO, Vec3::Y * (fallback * 0.9 / 0.02));
    let respawned = ReleaseFrame {
        rig: RigSample {
            rotation: Quat::from_rotation_y(fallback * 0.9 + fallback * 1.1),
            ..first.rig
        },
        ..first
    };
    assert_eq!(transport.update(Some(respawned), 0.02, Some(0.94)), None);
    assert_eq!(transport.history_after_s(), Some(0.94));
}
