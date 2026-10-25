use std::f32::consts::FRAC_PI_2;

use glam::{Mat3, Vec2};

use super::*;
use crate::controllers::body::rig::root::{
    ledger::{CompletedMovement, Movement, Requests},
    shift::CockpitShift,
};

fn epochs() -> Epochs {
    Epochs {
        origin: 3,
        calibration: 5,
        world_scale: 7,
        authority: 11,
    }
}

fn stamp() -> SampleStamp {
    SampleStamp {
        sequence: 23,
        display_time_ns: 456_000_000,
        published_at: Instant::now(),
        epochs: epochs(),
    }
}

fn tracked(position: Vec3) -> TrackedPose {
    TrackedPose {
        pose: Pose {
            position,
            orientation: Quat::IDENTITY,
        },
        position_valid: true,
        orientation_valid: true,
        position_tracked: true,
        orientation_tracked: true,
        ..TrackedPose::default()
    }
}

fn raw(stamp: SampleStamp) -> RawTracking {
    let located = |position| RawPose {
        stamp,
        tracked: tracked(position),
    };
    RawTracking {
        head: located(Vec3::new(0.3, 0.2, -0.4)),
        grips: [
            Some(located(Vec3::new(-0.2, -0.4, -0.6))),
            Some(located(Vec3::new(0.2, -0.4, -0.6))),
        ],
        focused: true,
    }
}

fn side(step: u64, root: Vec3, yaw: f32, shift: Vec3) -> CompositionSide {
    let rotation = Quat::from_rotation_y(yaw);
    CompositionSide {
        stamp: StepStamp {
            step,
            epochs: epochs(),
        },
        root: Mat4::from_rotation_translation(rotation, root),
        anchor_world: root + Vec3::Y * 1.6,
        basis: CockpitBasis {
            body: rotation,
            world_scale: 1.0,
        },
        shift_cockpit: shift,
        owner: RootOwner::Player,
        native_rig: None,
    }
}

fn pair() -> CompositionPair {
    CompositionPair::new(
        side(31, Vec3::ZERO, 0.0, Vec3::ZERO),
        side(32, Vec3::X, 0.0, Vec3::ZERO),
    )
    .unwrap()
}

fn near(actual: Vec3, expected: Vec3) {
    assert!(actual.distance(expected) < 1.0e-5, "{actual} != {expected}");
}

fn rotation_matches(actual: Quat, expected: Mat3) {
    assert!((actual.length_squared() - 1.0).abs() < 1.0e-6);
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        near(actual * axis, expected * axis);
    }
}

fn near_unit(rotation: Quat, scale: f32) -> Quat {
    Quat::from_xyzw(
        rotation.x * scale,
        rotation.y * scale,
        rotation.z * scale,
        rotation.w * scale,
    )
}

#[test]
fn previous_and_current_keep_their_anchor_basis_shift_and_root() {
    let mut previous = side(31, Vec3::X * 10.0, 0.0, Vec3::X * 0.1);
    let mut current = side(
        32,
        Vec3::new(20.0, 0.0, 3.0),
        FRAC_PI_2,
        Vec3::new(0.2, 0.0, 0.1),
    );
    previous.basis.world_scale = 2.0;
    current.basis.world_scale = 2.0;
    let observation = Observation::new(
        raw(stamp()),
        CompositionPair::new(previous, current).unwrap(),
    )
    .unwrap();

    for (which, expected_head, expected_grip, root) in [
        (
            PairSide::Previous,
            Vec3::new(10.4, 2.0, -0.8),
            Vec3::new(9.4, 0.8, -1.2),
            previous.root,
        ),
        (
            PairSide::Current,
            Vec3::new(19.0, 2.0, 2.8),
            Vec3::new(18.6, 0.8, 3.8),
            current.root,
        ),
    ] {
        let frames = observation.frames(which).unwrap();
        let head = observation.head_world(which).unwrap();
        let grip = observation.grip_world(Hand::Left, which).unwrap().unwrap();
        near(head.position, expected_head);
        near(grip.position, expected_grip);
        assert_eq!(frames.character, root);
        near(
            frames.world_to_cockpit(grip.position),
            Vec3::new(-0.2, -0.4, -0.6),
        );
        near(
            frames.cockpit_to_model(frames.head_cockpit),
            root.inverse().transform_point3(expected_head),
        );
        let basis = observation.pair().side(which).basis.body;
        assert!(head.orientation.dot(basis).abs() > 0.99999);
    }
}

#[test]
fn independent_anchor_basis_and_root_keep_noncommuting_pitch_roll_order() {
    let mut input = raw(stamp());
    input.head.tracked.pose.orientation = Quat::from_rotation_x(0.4) * Quat::from_rotation_z(-0.3);
    input.grips[0].as_mut().unwrap().tracked.pose.orientation =
        Quat::from_rotation_z(0.6) * Quat::from_rotation_x(-0.2);
    let previous_basis = Mat3::from_rotation_z(-0.5) * Mat3::from_rotation_y(0.7);
    let current_basis = Mat3::from_rotation_y(-0.3) * Mat3::from_rotation_x(0.8);
    let mut previous = side(31, Vec3::new(14.0, -3.0, 8.0), -1.0, Vec3::ZERO);
    let mut current = side(32, Vec3::new(-9.0, 6.0, -2.0), 0.2, Vec3::ZERO);
    previous.root = Mat4::from_rotation_translation(
        Quat::from_rotation_x(-0.6) * Quat::from_rotation_z(0.9),
        Vec3::new(14.0, -3.0, 8.0),
    );
    current.root = Mat4::from_rotation_translation(
        Quat::from_rotation_z(-0.4) * Quat::from_rotation_x(0.3),
        Vec3::new(-9.0, 6.0, -2.0),
    );
    previous.anchor_world = Vec3::new(-2.0, 4.0, 7.0);
    current.anchor_world = Vec3::new(5.0, -1.0, -3.0);
    previous.basis.body = Quat::from_rotation_z(-0.5) * Quat::from_rotation_y(0.7);
    current.basis.body = Quat::from_rotation_y(-0.3) * Quat::from_rotation_x(0.8);
    previous.basis.world_scale = 1.3;
    current.basis.world_scale = 1.3;
    previous.shift_cockpit = Vec3::new(0.1, -0.2, 0.3);
    current.shift_cockpit = Vec3::new(-0.2, 0.1, -0.1);
    let observation =
        Observation::new(input, CompositionPair::new(previous, current).unwrap()).unwrap();
    let head_rotation = Mat3::from_rotation_x(0.4) * Mat3::from_rotation_z(-0.3);
    let grip_rotation = Mat3::from_rotation_z(0.6) * Mat3::from_rotation_x(-0.2);
    for (which, composition, basis) in [
        (PairSide::Previous, previous, previous_basis),
        (PairSide::Current, current, current_basis),
    ] {
        let frames = observation.frames(which).unwrap();
        let head = observation.head_world(which).unwrap();
        let grip = observation.grip_world(Hand::Left, which).unwrap().unwrap();
        let expected_head = composition.anchor_world
            + basis * ((input.head.tracked.pose.position - composition.shift_cockpit) * 1.3);
        let expected_grip = composition.anchor_world
            + basis
                * ((input.grips[0].unwrap().tracked.pose.position - composition.shift_cockpit)
                    * 1.3);
        near(head.position, expected_head);
        near(grip.position, expected_grip);
        rotation_matches(head.orientation, basis * head_rotation);
        rotation_matches(grip.orientation, basis * grip_rotation);
        assert!((head.orientation * Vec3::Z).distance((head_rotation * basis) * Vec3::Z) > 0.05);
        assert_eq!(frames.character, composition.root);
        near(
            frames.cockpit_to_model(input.grips[0].unwrap().tracked.pose.position),
            composition.root.inverse().transform_point3(expected_grip),
        );
    }
}

#[test]
fn near_unit_inputs_produce_unit_outputs_and_rigid_cockpit_world_roundtrips() {
    for (basis, head_rotation, grip_rotation) in [
        (Quat::IDENTITY, Quat::IDENTITY, Quat::IDENTITY),
        (
            Quat::from_rotation_z(0.5) * Quat::from_rotation_x(-0.7),
            Quat::from_rotation_x(0.3) * Quat::from_rotation_y(-0.4),
            Quat::from_rotation_y(0.8) * Quat::from_rotation_z(-0.2),
        ),
    ] {
        for rounding_scale in [1.00004, 0.99996] {
            let mut input = raw(stamp());
            input.head.tracked.pose.orientation = near_unit(head_rotation, rounding_scale);
            for grip in input.grips.iter_mut().flatten() {
                grip.tracked.pose.orientation = near_unit(grip_rotation, rounding_scale);
            }
            let mut composition = side(32, Vec3::ZERO, 0.0, Vec3::new(0.1, -0.2, 0.3));
            composition.anchor_world = Vec3::new(2.0, -1.0, 3.0);
            composition.basis.body = near_unit(basis, rounding_scale);
            composition.basis.world_scale = 1.7;
            let pair = CompositionPair::new(composition, composition).unwrap();
            let observation = Observation::new(input, pair).unwrap();
            for which in [PairSide::Previous, PairSide::Current] {
                let frames = observation.frames(which).unwrap();
                let head = observation.head_world(which).unwrap();
                rotation_matches(frames.view, Mat3::from_quat(basis));
                rotation_matches(
                    head.orientation,
                    Mat3::from_quat(basis) * Mat3::from_quat(head_rotation),
                );
                near(
                    head.position,
                    composition.anchor_world
                        + basis
                            * ((input.head.tracked.pose.position - composition.shift_cockpit)
                                * 1.7),
                );
                for hand in Hand::BOTH {
                    let grip = observation.grip_world(hand, which).unwrap().unwrap();
                    rotation_matches(
                        grip.orientation,
                        Mat3::from_quat(basis) * Mat3::from_quat(grip_rotation),
                    );
                }
                for point in [Vec3::new(3.0, -2.0, 4.0), Vec3::new(-4.0, 3.0, -2.0)] {
                    let world = frames.cockpit_to_world(point);
                    near(frames.world_to_cockpit(world), point);
                    near(
                        world,
                        composition.anchor_world
                            + basis * ((point - composition.shift_cockpit) * 1.7),
                    );
                }
            }
            assert_eq!(observation.raw(), &input);
            assert_eq!(observation.pair(), &pair);
            assert_eq!(
                pair.side(PairSide::Current).basis.body,
                composition.basis.body
            );
        }
    }
}

#[test]
fn roomscale_completed_chase_preserves_composed_view() {
    for dt in [1.0 / 30.0, 1.0 / 90.0] {
        for scale in [0.8, 1.4] {
            for yaw in [0.0, 0.8] {
                for blocked in [false, true] {
                    let movement = Movement::new(Vec2::Y * 0.6, Vec2::X * 0.4, dt);
                    let actual = if blocked {
                        Vec3::ZERO
                    } else {
                        Vec3::new(movement.chase.x, 0.0, movement.task.y)
                    };
                    let mut requests = Requests::new();
                    requests.submit(movement);
                    requests.complete(Vec3::ZERO, actual);
                    let basis = CockpitBasis {
                        body: Quat::from_rotation_y(yaw),
                        world_scale: scale,
                    };
                    let mut shift = CockpitShift::new();
                    shift.observe(&basis, requests.completed());
                    let (negative_previous, negative_current) = shift.apply(Vec3::ZERO);
                    let mut previous = side(31, Vec3::ZERO, yaw, -negative_previous);
                    let mut current = side(32, actual, yaw, -negative_current);
                    previous.basis = basis;
                    current.basis = basis;
                    let observation = Observation::new(
                        raw(stamp()),
                        CompositionPair::new(previous, current).unwrap(),
                    )
                    .unwrap();
                    let first = observation.head_world(PairSide::Previous).unwrap();
                    let last = observation.head_world(PairSide::Current).unwrap();
                    let task_travel = if blocked {
                        Vec3::ZERO
                    } else {
                        Vec3::new(movement.task.x, 0.0, movement.task.y)
                    };
                    near(last.position, first.position + task_travel);
                    let expected = if blocked { Vec2::ZERO } else { movement.chase };
                    assert_eq!(requests.completed().current, expected);
                }
            }
        }
    }
}

#[test]
fn pending_and_blocked_requests_never_supply_completed_compensation() {
    let mut requests = Requests::new();
    let mut shift = CockpitShift::new();
    let movement = Movement::new(Vec2::ZERO, Vec2::X, 0.02);
    requests.submit(movement);
    assert!(requests.pending());
    assert_eq!(requests.completed(), CompletedMovement::default());
    let basis = side(31, Vec3::ZERO, 0.0, Vec3::ZERO).basis;
    shift.observe(&basis, requests.completed());
    assert_eq!(shift.apply(Vec3::ZERO), (Vec3::ZERO, Vec3::ZERO));
    requests.complete(Vec3::ZERO, Vec3::ZERO);
    shift.observe(&basis, requests.completed());
    assert_eq!(shift.observed(), Vec2::ZERO);
    let observation = Observation::new(
        raw(stamp()),
        CompositionPair::new(
            side(31, Vec3::ZERO, 0.0, Vec3::ZERO),
            side(32, Vec3::ZERO, 0.0, Vec3::ZERO),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        observation.head_world(PairSide::Previous).unwrap(),
        observation.head_world(PairSide::Current).unwrap()
    );
    assert!(!requests.pending());
}

#[test]
fn platform_transport_is_applied_once_to_raw_grips() {
    for owner in [RootOwner::Game, RootOwner::Seated, RootOwner::Authored] {
        for (translation, yaw) in [(0.1, 0.0), (0.5, 0.2), (1.4, -0.7)] {
            let mut previous = side(31, Vec3::new(8.0, 0.0, -2.0), 0.1, Vec3::ZERO);
            let mut current = side(32, Vec3::new(8.0 + translation, 0.0, -2.0), yaw, Vec3::ZERO);
            previous.owner = owner;
            current.owner = owner;
            let observation = Observation::new(
                raw(stamp()),
                CompositionPair::new(previous, current).unwrap(),
            )
            .unwrap();
            for hand in Hand::BOTH {
                let local = observation.raw().grips[hand.index()]
                    .unwrap()
                    .tracked
                    .pose
                    .position;
                let expected_previous = previous.anchor_world + previous.basis.body * local;
                let expected_current = current.anchor_world + current.basis.body * local;
                for _ in 0..3 {
                    let first = observation
                        .grip_world(hand, PairSide::Previous)
                        .unwrap()
                        .unwrap();
                    let last = observation
                        .grip_world(hand, PairSide::Current)
                        .unwrap()
                        .unwrap();
                    near(first.position, expected_previous);
                    near(last.position, expected_current);
                }
            }
            assert_eq!(observation.raw().head.stamp.sequence, 23);
            assert_eq!(observation.pair().side(PairSide::Current).stamp.step, 32);
        }
    }
}

#[test]
fn head_and_grips_must_share_the_complete_sample_stamp() {
    let input = raw(stamp());
    let mut other_sequence = input.head.stamp;
    other_sequence.sequence += 1;
    let mut other_time = input.head.stamp;
    other_time.display_time_ns += 1;
    let mut other_publication = input.head.stamp;
    other_publication.published_at += Duration::from_nanos(1);
    let mut other_origin = input.head.stamp;
    other_origin.epochs.origin += 1;
    for mismatched in [other_sequence, other_time, other_publication, other_origin] {
        let mut mixed = input;
        mixed.grips[1].as_mut().unwrap().stamp = mismatched;
        assert_eq!(
            Observation::new(mixed, pair()).unwrap_err(),
            ObservationError::MixedSamples
        );
    }
}

#[test]
fn epoch_changes_reject_cross_epoch_pairs_packets_and_consumption() {
    let input = raw(stamp());
    let original = pair();
    let mut changed = [epochs(); 4];
    changed[0].origin += 1;
    changed[1].calibration += 1;
    changed[2].world_scale += 1;
    changed[3].authority += 1;
    for next in changed {
        let mut previous = *original.side(PairSide::Previous);
        let mut current = *original.side(PairSide::Current);
        current.stamp.epochs = next;
        assert_eq!(
            CompositionPair::new(previous, current).unwrap_err(),
            ObservationError::EpochMismatch
        );
        previous.stamp.epochs = next;
        let rebased = CompositionPair::new(previous, current).unwrap();
        assert_eq!(
            Observation::new(input, rebased).unwrap_err(),
            ObservationError::EpochMismatch
        );
        let observation = Observation::new(input, original).unwrap();
        assert_eq!(
            observation.check_at(input.head.stamp.published_at, Duration::from_secs(1), next),
            Err(ObservationError::EpochMismatch)
        );
    }
}

fn rebase_raw_pose(located: RawPose, stamp: SampleStamp, turn: Quat) -> RawPose {
    RawPose {
        stamp,
        tracked: TrackedPose {
            pose: Pose {
                position: turn * located.tracked.pose.position,
                orientation: turn * located.tracked.pose.orientation,
            },
            ..located.tracked
        },
    }
}

#[test]
fn coordinate_rebase_preserves_world_poses_with_nonzero_shift() {
    let input = raw(stamp());
    let mut next_input = input;
    let mut next_stamp = input.head.stamp;
    next_stamp.sequence += 1;
    next_stamp.epochs.origin += 1;
    let turn = Quat::from_rotation_y(-0.7);
    next_input.head = rebase_raw_pose(input.head, next_stamp, turn);
    next_input.grips = input
        .grips
        .map(|grip| grip.map(|located| rebase_raw_pose(located, next_stamp, turn)));
    let old_side = side(32, Vec3::new(8.0, 0.0, -3.0), 0.2, Vec3::X * 0.3);
    let mut next_side = old_side;
    next_side.stamp.epochs = next_stamp.epochs;
    next_side.basis.body = old_side.basis.body * turn.inverse();
    next_side.shift_cockpit = turn * old_side.shift_cockpit;
    let old = Observation::new(input, CompositionPair::new(old_side, old_side).unwrap()).unwrap();
    let rebased = Observation::new(
        next_input,
        CompositionPair::new(next_side, next_side).unwrap(),
    )
    .unwrap();
    for which in [PairSide::Previous, PairSide::Current] {
        let head = old.head_world(which).unwrap();
        let next_head = rebased.head_world(which).unwrap();
        near(next_head.position, head.position);
        assert!(next_head.orientation.dot(head.orientation).abs() > 0.99999);
        for hand in Hand::BOTH {
            near(
                rebased.grip_world(hand, which).unwrap().unwrap().position,
                old.grip_world(hand, which).unwrap().unwrap().position,
            );
        }
    }
    assert_eq!(
        old.check_at(next_stamp.published_at, Duration::ZERO, next_stamp.epochs),
        Err(ObservationError::EpochMismatch)
    );
}

#[test]
fn freshness_uses_host_time_and_reads_do_not_refresh_the_sample() {
    let mut input = raw(stamp());
    input.head.stamp.display_time_ns = -900_000;
    for grip in input.grips.iter_mut().flatten() {
        grip.stamp = input.head.stamp;
    }
    let observation = Observation::new(input, pair()).unwrap();
    let published = input.head.stamp.published_at;
    let limit = Duration::from_millis(40);
    for _ in 0..3 {
        assert_eq!(
            observation.check_at(published + limit, limit, epochs()),
            Ok(())
        );
        assert_eq!(
            observation.check_at(published + limit + Duration::from_nanos(1), limit, epochs()),
            Err(ObservationError::StaleSample)
        );
        assert_eq!(observation.raw().head.stamp, input.head.stamp);
    }
    assert_eq!(
        observation.check_at(published, Duration::ZERO, epochs()),
        Ok(())
    );
}

#[test]
fn future_publication_is_rejected_by_order_even_within_clock_error_tolerance() {
    let now = Instant::now();
    for offset in [
        Duration::from_nanos(1),
        Duration::from_micros(1),
        Duration::from_millis(1),
        Duration::from_secs(1),
    ] {
        let published_at = now.checked_add(offset).unwrap();
        assert!(now < published_at);
        let mut sample = stamp();
        sample.published_at = published_at;
        let observation = Observation::new(raw(sample), pair()).unwrap();
        assert_eq!(
            observation.check_at(now, Duration::MAX, epochs()),
            Err(ObservationError::FuturePublication),
            "future publication offset: {offset:?}"
        );
        assert_eq!(
            observation.check_at(published_at, Duration::ZERO, epochs()),
            Ok(())
        );
    }
}

#[test]
fn invalid_head_blocks_mapping_without_fabricating_a_pose() {
    let mut input = raw(stamp());
    input.head.tracked.position_valid = false;
    input.head.tracked.pose.position = Vec3::NAN;
    let observation = Observation::new(input, pair()).unwrap();
    assert_eq!(
        observation.frames(PairSide::Current),
        Err(ObservationError::InvalidHead)
    );
    assert_eq!(
        observation.head_world(PairSide::Previous),
        Err(ObservationError::InvalidHead)
    );
    assert_eq!(
        observation.check_at(input.head.stamp.published_at, Duration::ZERO, epochs()),
        Err(ObservationError::InvalidHead)
    );
    assert!(!observation.raw().head.tracked.position_valid);
}

#[test]
fn hands_lose_validity_independently_and_runtime_inference_stays_labeled() {
    let mut input = raw(stamp());
    let left = input.grips[0].as_mut().unwrap();
    left.tracked.orientation_valid = false;
    left.tracked.pose.orientation = Quat::from_xyzw(f32::NAN, 0.0, 0.0, 0.0);
    let right = input.grips[1].as_mut().unwrap();
    right.tracked.position_tracked = false;
    right.tracked.orientation_tracked = false;
    let observation = Observation::new(input, pair()).unwrap();
    assert_eq!(
        observation
            .grip_world(Hand::Left, PairSide::Current)
            .unwrap(),
        None
    );
    assert!(
        observation
            .grip_world(Hand::Right, PairSide::Current)
            .unwrap()
            .is_some()
    );
    assert!(!observation.raw().grips[1].unwrap().tracked.is_tracked());
    assert_eq!(
        observation.check_at(input.head.stamp.published_at, Duration::ZERO, epochs()),
        Ok(())
    );
    input.grips[0] = None;
    input.focused = false;
    let unfocused = Observation::new(input, pair()).unwrap();
    assert_eq!(unfocused.raw().grips[0], None);
    assert_eq!(
        unfocused.check_at(input.head.stamp.published_at, Duration::ZERO, epochs()),
        Err(ObservationError::Unfocused)
    );
}

#[test]
fn nonfinite_valid_tracking_and_degenerate_or_projective_frames_are_rejected() {
    let input = raw(stamp());
    for bad in [
        TrackedPose {
            pose: Pose {
                position: Vec3::NAN,
                ..Pose::IDENTITY
            },
            ..tracked(Vec3::ZERO)
        },
        TrackedPose {
            pose: Pose {
                orientation: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
                ..Pose::IDENTITY
            },
            ..tracked(Vec3::ZERO)
        },
        TrackedPose {
            linear_velocity: Some(Vec3::NAN),
            ..tracked(Vec3::ZERO)
        },
        TrackedPose {
            angular_velocity: Some(Vec3::NAN),
            ..tracked(Vec3::ZERO)
        },
    ] {
        let mut invalid = input;
        invalid.head.tracked = bad;
        assert_eq!(
            Observation::new(invalid, pair()).unwrap_err(),
            ObservationError::InvalidTracking
        );
    }
    let good = side(31, Vec3::ZERO, 0.0, Vec3::ZERO);
    let mut projective = good;
    projective.root.x_axis.w = 0.2;
    for bad in [
        CompositionSide {
            root: Mat4::ZERO,
            ..good
        },
        CompositionSide {
            anchor_world: Vec3::NAN,
            ..good
        },
        CompositionSide {
            shift_cockpit: Vec3::NAN,
            ..good
        },
        CompositionSide {
            basis: CockpitBasis {
                world_scale: 0.0,
                ..good.basis
            },
            ..good
        },
        CompositionSide {
            basis: CockpitBasis {
                body: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
                ..good.basis
            },
            ..good
        },
        projective,
    ] {
        assert_eq!(
            CompositionPair::new(bad, bad).unwrap_err(),
            ObservationError::InvalidComposition
        );
    }
}

#[test]
fn composed_output_overflow_is_rejected() {
    let mut input = raw(stamp());
    input.head.tracked.pose.position = Vec3::ZERO;
    input.grips[0].as_mut().unwrap().tracked.pose.position = Vec3::splat(f32::MAX);
    let mut composition = side(32, Vec3::ZERO, 0.0, Vec3::ZERO);
    composition.basis.world_scale = 2.0;
    let pair = CompositionPair::new(composition, composition).unwrap();
    let observation = Observation::new(input, pair).unwrap();
    assert_eq!(
        observation.grip_world(Hand::Left, PairSide::Current),
        Err(ObservationError::InvalidComposition)
    );
    input.head.tracked.pose.position = Vec3::splat(f32::MAX);
    let observation = Observation::new(input, pair).unwrap();
    assert_eq!(
        observation.head_world(PairSide::Current),
        Err(ObservationError::InvalidComposition)
    );
}

#[test]
fn reversed_steps_changed_scale_and_unversioned_owner_changes_are_rejected() {
    let previous = side(31, Vec3::ZERO, 0.0, Vec3::ZERO);
    let current = side(32, Vec3::ZERO, 0.0, Vec3::ZERO);
    assert_eq!(
        CompositionPair::new(current, previous).unwrap_err(),
        ObservationError::ReversedSteps
    );
    let mut changed = current;
    changed.basis.world_scale = 1.1;
    assert_eq!(
        CompositionPair::new(previous, changed).unwrap_err(),
        ObservationError::ScaleMismatch
    );
    changed = current;
    changed.owner = RootOwner::Game;
    assert_eq!(
        CompositionPair::new(previous, changed).unwrap_err(),
        ObservationError::OwnerChangedWithoutEpoch
    );
    assert!(CompositionPair::new(current, current).is_ok());
    changed = current;
    changed.anchor_world += Vec3::X;
    assert_eq!(
        CompositionPair::new(current, changed).unwrap_err(),
        ObservationError::UncollapsedStep
    );
}

#[test]
fn native_rig_is_separate_from_raw_tracking_and_keeps_its_step_side() {
    let landmarks = SkeletonLandmarks {
        head: Vec3::Y * 1.7,
        pelvis: Vec3::Y,
        hips: [Vec3::new(-0.1, 1.0, 0.0), Vec3::new(0.1, 1.0, 0.0)],
        shoulders: [Vec3::new(-0.2, 1.5, 0.0), Vec3::new(0.2, 1.5, 0.0)],
    };
    let base = pair();
    let input = raw(stamp());
    let before = Observation::new(input, base).unwrap();
    let mut previous = *base.side(PairSide::Previous);
    let mut current = *base.side(PairSide::Current);
    previous.native_rig = Some(landmarks);
    current.native_rig = Some(landmarks.map(|p| p + Vec3::X * 0.1));
    let observation =
        Observation::new(input, CompositionPair::new(previous, current).unwrap()).unwrap();
    assert_eq!(observation.raw(), before.raw());
    for which in [PairSide::Previous, PairSide::Current] {
        assert_eq!(observation.head_world(which), before.head_world(which));
    }
    assert_ne!(
        observation.pair().side(PairSide::Previous).native_rig,
        observation.pair().side(PairSide::Current).native_rig
    );
    current.native_rig.as_mut().unwrap().head = Vec3::NAN;
    assert_eq!(
        CompositionPair::new(previous, current).unwrap_err(),
        ObservationError::InvalidNativeRig
    );
}
