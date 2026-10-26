use std::{mem, time::Instant};

use super::*;
use crate::controllers::frame::{ActionSetKind, HandFrame, TrackedPose};

fn record(hand: Hand, weapon: usize) -> ReleaseRecord {
    ReleaseRecord {
        sequence: 0,
        hand,
        weapon: Some(weapon),
        route: ReleaseRoute::Wielded,
        max_speed_mps: 15.0,
        controller: None,
        stages: ReleaseStages::default(),
        drop: None,
        refusal: None,
        before_disarm_sample: Unavailable::NoExistingObservation,
        native_readback: NativeReadback::UNAVAILABLE,
    }
}

fn observation(stage: SampleStage, motion: Option<(Vec3, Vec3)>) -> DropObservation {
    DropObservation::sampled(
        42,
        Some(WeaponSlot::E_SLOT_TWO_HANDED),
        stage,
        motion,
        Some(7),
    )
}

#[test]
fn cached_motion_retains_each_vectors_validity_without_substituting_zero() {
    let cases = [
        (Vec3::ZERO, Vec3::ZERO, true, true),
        (Vec3::X, Vec3::Y, true, true),
        (Vec3::splat(f32::NAN), Vec3::Y, false, true),
        (Vec3::X, Vec3::splat(f32::INFINITY), true, false),
    ];
    for (linear, angular, linear_valid, angular_valid) in cases {
        let sample = observation(SampleStage::Wielded, Some((linear, angular)))
            .cached_world_motion
            .unwrap();
        assert_eq!(sample.linear_is_finite(), linear_valid);
        assert_eq!(sample.angular_is_finite(), angular_valid);
        if linear_valid {
            assert_eq!(sample.linear, linear);
        }
        if angular_valid {
            assert_eq!(sample.angular, angular);
        }
    }
    assert!(
        observation(SampleStage::Wielded, None)
            .cached_world_motion
            .is_none()
    );
}

#[test]
fn a_recent_controller_publication_does_not_date_the_cached_gun_sample() {
    let published_at = Instant::now() - Duration::from_millis(20);
    let mut hands: [HandFrame; 2] = Default::default();
    hands[Hand::Left.index()].active = true;
    hands[Hand::Left.index()].grip = TrackedPose {
        position_valid: true,
        orientation_valid: true,
        position_tracked: false,
        orientation_tracked: true,
        linear_velocity: Some(Vec3::Y),
        angular_velocity: None,
        ..TrackedPose::default()
    };
    let frame = ControllerFrame {
        sequence: 13,
        display_time_ns: 100,
        published_at,
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands,
    };
    let context = ControllerContext::from_frame(&frame, Hand::Left);
    assert_eq!(context.frame_sequence, 13);
    assert_eq!(context.display_time_ns, 100);
    assert!(context.publication_age >= Duration::from_millis(20));
    assert!(context.active && context.position_valid && context.orientation_valid);
    assert!(!context.position_tracked);
    assert!(context.orientation_tracked);
    assert_eq!(context.linear_cockpit_mps, Some(Vec3::Y));
    assert_eq!(context.angular_cockpit_rad_s, None);
    assert!(!ControllerContext::from_frame(&frame, Hand::Right).active);

    let sample = observation(SampleStage::AfterDisarm, Some((Vec3::X, Vec3::ZERO)));
    assert_eq!(sample.sample_age, Unavailable::ProducerTimeUnknown);
    assert_eq!(
        sample.sample_source,
        SampleSource::CachedWeaponDataWorldMatrices
    );
    assert_eq!(sample.cached_world_motion.unwrap().linear, Vec3::X);
    assert_eq!(sample.requested_world_motion, None);
}

#[test]
fn initial_stages_do_not_claim_disarm_drop_or_readback() {
    let attempt = ReleaseRecord::new(Hand::Right, 0.0);
    assert_eq!(attempt.hand, Hand::Right);
    assert_eq!(attempt.weapon, None);
    assert_eq!(attempt.route, ReleaseRoute::Unresolved);
    assert_eq!(attempt.stages, ReleaseStages::default());
    assert_eq!(attempt.drop, None);
    assert_eq!(
        attempt.before_disarm_sample,
        Unavailable::NoExistingObservation
    );
    assert_eq!(attempt.native_readback, NativeReadback::UNAVAILABLE);

    for stage in [SampleStage::Wielded, SampleStage::AfterDisarm] {
        let sample = observation(stage, None);
        assert_eq!(sample.sample_stage, stage);
        assert!(!sample.on_drop_returned);
        assert!(!sample.slot_clear_returned);
        assert_eq!(sample.write, NativeWrite::NotAttempted);
    }
}

#[test]
fn setter_completion_preserves_requested_values_without_claiming_readback() {
    let mut attempt = record(Hand::Right, 42);
    let mut sample = observation(
        SampleStage::Wielded,
        Some((Vec3::splat(f32::INFINITY), Vec3::Y)),
    );
    sample.requested_world_motion = Some(WorldMotion {
        linear: Vec3::ZERO,
        angular: Vec3::Y,
    });
    sample.write = NativeWrite::SetterReturned;
    sample.on_drop_returned = true;
    sample.slot_clear_returned = true;
    attempt.drop = Some(sample);
    attempt.stages.slot_clear_returned = true;
    attempt.stages.reconcile_returned = true;
    let mut status = ReleaseStatus::EMPTY;
    status.record(attempt);
    let saved = status.latest.unwrap();
    let drop = saved.drop.unwrap();
    assert!(!drop.cached_world_motion.unwrap().linear_is_finite());
    assert_eq!(drop.requested_world_motion.unwrap().linear, Vec3::ZERO);
    assert_eq!(drop.write, NativeWrite::SetterReturned);
    assert!(saved.stages.slot_clear_returned && saved.stages.reconcile_returned);
    assert_eq!(
        saved.native_readback.after_setter,
        Unavailable::ReadbackContractUnproven
    );
    assert_eq!(
        saved.native_readback.after_slot_clear,
        Unavailable::LifetimeAndPhaseUnproven
    );
    assert_eq!(
        saved.native_readback.next_step,
        Unavailable::LifetimeAndPhaseUnproven
    );
}

#[test]
fn a_refused_other_hand_attempt_replaces_the_previous_complete_release() {
    let mut status = ReleaseStatus::EMPTY;
    let mut first = record(Hand::Left, 42);
    first.drop = Some(observation(SampleStage::Wielded, None));
    first.stages.slot_clear_returned = true;
    first.stages.reconcile_returned = true;
    status.record(first);
    let previous_copy = status.latest.unwrap();

    let mut second = record(Hand::Right, 99);
    second.route = ReleaseRoute::ArmedInHand;
    second.stages.before_disarm = true;
    second.stages.disarm_returned = true;
    second.refusal = Some(DockRefusal::NotHolstered);
    status.record(second);
    let latest = status.latest.unwrap();
    assert_eq!(latest.sequence, 2);
    assert_eq!(latest.hand, Hand::Right);
    assert_eq!(latest.weapon, Some(99));
    assert_eq!(latest.route, ReleaseRoute::ArmedInHand);
    assert_eq!(latest.refusal, Some(DockRefusal::NotHolstered));
    assert!(latest.stages.before_disarm && latest.stages.disarm_returned);
    assert!(!latest.stages.slot_clear_returned && !latest.stages.reconcile_returned);
    assert_eq!(latest.drop, None);
    assert_eq!(previous_copy.weapon, Some(42));
    assert_eq!(previous_copy.hand, Hand::Left);
    assert_eq!(previous_copy.sequence, 1);
}

#[test]
fn copied_native_and_loadout_identities_remain_distinct() {
    let mut attempt = record(Hand::Left, 99);
    attempt.drop = Some(observation(SampleStage::Wielded, None));
    let mut status = ReleaseStatus::EMPTY;
    status.record(attempt);
    let latest = status.latest.unwrap();
    assert_eq!(latest.weapon, Some(99));
    assert_eq!(latest.drop.unwrap().weapon, 42);
    assert_eq!(latest.drop.unwrap().native_body_id, Some(7));
}

#[test]
fn latest_record_storage_is_fixed_size_across_many_attempts() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<ReleaseStatus>();
    assert!(mem::size_of::<ReleaseStatus>() <= 512);
    assert!(!mem::needs_drop::<ReleaseStatus>());
    let mut status = ReleaseStatus::EMPTY;
    for weapon in 1..=10_000 {
        status.record(record(Hand::Left, weapon));
    }
    assert_eq!(status.attempts, 10_000);
    assert_eq!(status.latest.unwrap().weapon, Some(10_000));
    assert_eq!(status.latest.unwrap().sequence, 10_000);
}

#[test]
fn heavy_write_rejection_remains_distinct_from_a_zero_request() {
    let mut status = ReleaseStatus::EMPTY;
    let mut attempt = record(Hand::Left, 42);
    let mut sample = observation(SampleStage::Wielded, Some((Vec3::ZERO, Vec3::ZERO)));
    sample.slot = Some(WeaponSlot::E_SLOT_HEAVY);
    sample.slot_clear_returned = true;
    assert_eq!(sample.requested_world_motion, None);
    assert!(!sample.on_drop_returned);

    sample.branch = DropBranch::OnDropThenVelocityWrite;
    sample.requested_world_motion = Some(WorldMotion {
        linear: Vec3::ZERO,
        angular: Vec3::ZERO,
    });
    sample.write = NativeWrite::Rejected(BodyError::Reowned);
    sample.on_drop_returned = true;
    attempt.drop = Some(sample);
    status.record(attempt);
    let rejected = status.latest.unwrap().drop.unwrap();
    assert_eq!(rejected.write, NativeWrite::Rejected(BodyError::Reowned));
    assert_eq!(rejected.requested_world_motion.unwrap().linear, Vec3::ZERO);
    assert_eq!(
        status.latest.unwrap().native_readback,
        NativeReadback::UNAVAILABLE
    );
}

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
        rotation: Quat::IDENTITY,
        scale: 1.0,
        teleport_epoch: 0,
        recenters: 0,
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
fn native_request_uses_timestamped_hand_motion_for_both_hands_and_supported_slots() {
    for hand in Hand::BOTH {
        for slot in [WeaponSlot::E_SLOT_TWO_HANDED, WeaponSlot::E_SLOT_HEAVY] {
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
            let release = estimate_release(ReleaseInput {
                rig: Some(rig),
                controller: Some(&context),
                samples: &samples,
                since_s: 0.9,
                history_after_s: None,
                point_world: rig.frame.origin,
            });
            let mut drop = observation(SampleStage::Wielded, Some((Vec3::ZERO, Vec3::ZERO)));
            drop.slot = Some(slot);
            let requested = drop.prepare_release(release, 20.0);
            assert_motion(requested, Vec3::new(3.0, 0.0, -8.0), Vec3::Y * 4.0);
            assert_eq!(drop.release_source, SampleSource::TimestampedHandHistory);
            assert_eq!(drop.history_age_s, Some(0.0));
            assert_eq!(drop.requested_world_motion, Some(requested));
        }
    }
}

#[test]
fn release_converts_point_motion_and_scale_once_and_rig_motion_once() {
    let context = controller(Hand::Left);
    let rig = RigMotion {
        frame: ReleaseFrame {
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            scale: 2.0,
            ..release_frame()
        },
        motion: WorldMotion {
            linear: Vec3::X * 5.0,
            angular: Vec3::Y,
        },
    };
    let point = rig.frame.origin + rig.frame.rotation * (Vec3::NEG_Z * 0.2);
    let sample = release_sample(1.0, Vec3::X * 2.0, Vec3::Y * 3.0);
    let release = estimate_release(ReleaseInput {
        rig: Some(rig),
        controller: Some(&context),
        samples: &[sample],
        since_s: 0.9,
        history_after_s: None,
        point_world: point,
    });
    let hand_linear = rig.frame.rotation * (Vec3::X * 1.7) * 2.0;
    let transport = rig.motion.linear + rig.motion.angular.cross(point - rig.frame.origin);
    assert_motion(release.world, hand_linear + transport, Vec3::Y * 4.0);
    let mut drop = observation(
        SampleStage::AfterDisarm,
        Some((Vec3::splat(900.0), Vec3::splat(900.0))),
    );
    let requested = drop.prepare_release(release, 2.0);
    assert!((requested.linear.length() - 2.0).abs() < 1e-3);
    assert!(
        requested
            .linear
            .normalize()
            .distance(release.world.linear.normalize())
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
        let release = estimate_release(ReleaseInput {
            rig: Some(rig),
            controller: Some(&invalid),
            samples: &samples,
            since_s: 0.0,
            history_after_s: None,
            point_world: point,
        });
        assert_eq!(release.source, SampleSource::RigTransportOnly);
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
    let release = estimate_release(ReleaseInput {
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
    let release = estimate_release(ReleaseInput {
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
fn a_missing_runtime_angular_velocity_still_moves_the_gun_point() {
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
    let release = estimate_release(ReleaseInput {
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
            recenters: 1,
            ..next
        },
        ReleaseFrame { scale: 2.0, ..next },
        ReleaseFrame {
            rotation: Quat::from_rotation_y(1.0),
            ..next
        },
        ReleaseFrame {
            origin: Vec3::splat(f32::NAN),
            ..next
        },
    ] {
        let mut transport = ReleaseTransport {
            previous: Some(next),
            ..ReleaseTransport::new()
        };
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
        rotation: Quat::from_rotation_y(0.4),
        scale: 1.5,
        ..release_frame()
    };
    let origin = base.origin + Vec3::new(0.5, 0.2, -0.1);
    let head_cockpit = Vec3::new(0.25, 0.1, -0.3);
    let head_world = origin + base.rotation * (head_cockpit * base.scale);
    let frame = base.with_composed_head(head_world, head_cockpit);
    assert!(frame.origin.distance(origin) < 1e-4);
    let sample = MotionSample {
        position: Vec3::new(0.3, -0.1, -0.4),
        orientation: Quat::from_rotation_y(0.35),
        ..release_sample(1.0, Vec3::ZERO, Vec3::Y * 4.0)
    };
    let lever = sample.orientation * (Vec3::NEG_Z * 0.2);
    let point = origin + frame.rotation * ((sample.position + lever) * frame.scale);
    let release = estimate_release(ReleaseInput {
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
    assert_eq!(release.source, SampleSource::TimestampedHandHistory);
    assert_motion(
        release.world,
        frame.rotation * (Vec3::Y * 4.0).cross(lever) * frame.scale,
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
        rotation: Quat::from_rotation_y(0.08),
        ..base
    };
    let current = current.with_composed_head(
        current_origin + current.rotation * current_head,
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
    let lever = current.rotation * sample.position;
    let context = controller(Hand::Left);
    let release = estimate_release(ReleaseInput {
        rig: Some(rig),
        controller: Some(&context),
        samples: &[sample],
        since_s: 0.9,
        history_after_s: transport.history_after_s(),
        point_world: current_origin + lever,
    });
    assert_eq!(release.source, SampleSource::TimestampedHandHistory);
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
            recenters: 1,
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
        let release = estimate_release(ReleaseInput {
            rig: Some(rig),
            controller: Some(&context),
            samples: &samples,
            since_s: 0.8,
            history_after_s: transport.history_after_s(),
            point_world: rig.frame.origin + Vec3::X * 0.5,
        });
        assert_eq!(release.source, SampleSource::TimestampedHandHistory);
        assert_motion(release.world, Vec3::ZERO, Vec3::ZERO);
        assert_eq!(release.history_age_s, Some(0.0));

        let acquired_later = estimate_release(ReleaseInput {
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

        let no_current_samples = estimate_release(ReleaseInput {
            rig: Some(rig),
            controller: Some(&context),
            samples: &samples[..2],
            since_s: 0.8,
            history_after_s: transport.history_after_s(),
            point_world: rig.frame.origin,
        });
        assert_eq!(no_current_samples.source, SampleSource::RigTransportOnly);
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
    let release = estimate_release(ReleaseInput {
        rig: None,
        controller: Some(&context),
        samples: &[release_sample(1.0, Vec3::X * 40.0, Vec3::Y * 20.0)],
        since_s: 0.0,
        history_after_s: None,
        point_world: Vec3::ZERO,
    });
    assert_eq!(release, ReleaseMotion::REST);
    for cap in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut drop = observation(
            SampleStage::OutsideInventory,
            Some((Vec3::X * 50.0, Vec3::Y * 50.0)),
        );
        let requested = drop.prepare_release(
            ReleaseMotion {
                world: WorldMotion {
                    linear: Vec3::X * 4.0,
                    angular: Vec3::splat(f32::NAN),
                },
                ..ReleaseMotion::REST
            },
            cap,
        );
        assert_motion(requested, Vec3::ZERO, Vec3::ZERO);
    }
}

#[test]
fn the_attempt_counter_saturates_without_preventing_replacement() {
    let mut status = ReleaseStatus {
        attempts: u64::MAX,
        latest: None,
    };
    status.record(record(Hand::Right, 42));
    status.record(record(Hand::Left, 99));
    assert_eq!(status.attempts, u64::MAX);
    assert_eq!(status.latest.unwrap().sequence, u64::MAX);
    assert_eq!(status.latest.unwrap().weapon, Some(99));
}
