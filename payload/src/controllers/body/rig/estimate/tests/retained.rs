//! Exact retained-input identity, including unavailable raw pose components.

use super::*;
use crate::controllers::body::rig::{frames::SkeletonLandmarks, observation::PairSide};

pub(crate) fn same_observation_bits(
    left: Option<&Observation>,
    right: Option<&Observation>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            let left_raw = left.raw();
            let right_raw = right.raw();
            left_raw.focused == right_raw.focused
                && same_raw_bits(left_raw.head, right_raw.head)
                && left_raw
                    .grips
                    .into_iter()
                    .zip(right_raw.grips)
                    .all(|(left, right)| match (left, right) {
                        (Some(left), Some(right)) => same_raw_bits(left, right),
                        (None, None) => true,
                        _ => false,
                    })
                && [PairSide::Previous, PairSide::Current]
                    .into_iter()
                    .all(|side| same_side_bits(*left.pair().side(side), *right.pair().side(side)))
        }
        (None, None) => true,
        _ => false,
    }
}

pub(crate) fn same_support_bits(left: Option<Support>, right: Option<Support>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.identity == right.identity
                && left.generation == right.generation
                && left.previous_step == right.previous_step
                && left.current_step == right.current_step
                && left.previous.to_cols_array().map(f32::to_bits)
                    == right.previous.to_cols_array().map(f32::to_bits)
                && left.current.to_cols_array().map(f32::to_bits)
                    == right.current.to_cols_array().map(f32::to_bits)
                && left.half_extent.to_array().map(f32::to_bits)
                    == right.half_extent.to_array().map(f32::to_bits)
        }
        (None, None) => true,
        _ => false,
    }
}

fn same_raw_bits(left: RawPose, right: RawPose) -> bool {
    left.stamp == right.stamp && same_tracked_bits(left.tracked, right.tracked)
}

fn same_tracked_bits(left: TrackedPose, right: TrackedPose) -> bool {
    left.position_valid == right.position_valid
        && left.orientation_valid == right.orientation_valid
        && left.position_tracked == right.position_tracked
        && left.orientation_tracked == right.orientation_tracked
        && same_vector_bits(left.pose.position, right.pose.position)
        && left.pose.orientation.to_array().map(f32::to_bits)
            == right.pose.orientation.to_array().map(f32::to_bits)
        && left
            .linear_velocity
            .map(|value| value.to_array().map(f32::to_bits))
            == right
                .linear_velocity
                .map(|value| value.to_array().map(f32::to_bits))
        && left
            .angular_velocity
            .map(|value| value.to_array().map(f32::to_bits))
            == right
                .angular_velocity
                .map(|value| value.to_array().map(f32::to_bits))
}

fn same_vector_bits(left: Vec3, right: Vec3) -> bool {
    left.to_array().map(f32::to_bits) == right.to_array().map(f32::to_bits)
}

fn same_side_bits(left: CompositionSide, right: CompositionSide) -> bool {
    left.stamp == right.stamp
        && left.root.to_cols_array().map(f32::to_bits)
            == right.root.to_cols_array().map(f32::to_bits)
        && same_vector_bits(left.anchor_world, right.anchor_world)
        && left.basis.body.to_array().map(f32::to_bits)
            == right.basis.body.to_array().map(f32::to_bits)
        && left.basis.world_scale.to_bits() == right.basis.world_scale.to_bits()
        && same_vector_bits(left.shift_cockpit, right.shift_cockpit)
        && left.owner == right.owner
        && match (left.native_rig, right.native_rig) {
            (Some(left), Some(right)) => same_landmarks_bits(left, right),
            (None, None) => true,
            _ => false,
        }
}

fn same_landmarks_bits(left: SkeletonLandmarks, right: SkeletonLandmarks) -> bool {
    same_vector_bits(left.head, right.head)
        && same_vector_bits(left.pelvis, right.pelvis)
        && left
            .hips
            .into_iter()
            .zip(right.hips)
            .all(|(left, right)| same_vector_bits(left, right))
        && left
            .shoulders
            .into_iter()
            .zip(right.shoulders)
            .all(|(left, right)| same_vector_bits(left, right))
}

#[test]
fn retained_input_identity_distinguishes_unavailable_nan_signs_and_payloads() {
    let fixture = Fixture::new();
    let original = fixture.observation(Sample::standing(1, 0.01));
    let positive_nan = f32::from_bits(0x7fc0_1234);
    let negative_nan = f32::from_bits(0xffc0_5678);
    let mut raw = *original.raw();
    let mut unavailable = raw.head;
    unavailable.tracked.position_valid = false;
    unavailable.tracked.orientation_valid = false;
    unavailable.tracked.position_tracked = false;
    unavailable.tracked.orientation_tracked = false;
    unavailable.tracked.pose.position = Vec3::splat(positive_nan);
    unavailable.tracked.pose.orientation = Quat::from_array([negative_nan; 4]);
    unavailable.tracked.linear_velocity = Some(Vec3::new(-0.0, 0.25, -0.5));
    unavailable.tracked.angular_velocity = Some(Vec3::new(0.0, -0.25, 0.5));
    raw.grips[0] = Some(unavailable);
    let observation = Observation::new(raw, *original.pair()).unwrap();
    let support = fixture.support(0, 1);
    let mut optimized = Estimator::new();
    let mut reference = Estimator::new();
    reference.set_reference_mode_for_test(true);
    let update = || Update {
        observation: &observation,
        now: fixture.now,
        max_age: Duration::from_millis(100),
        dt: 0.01,
        anatomy: &fixture.anatomy,
        support: Some(support),
        transition: Transition::Continue,
    };
    let actual = optimized.update(update());
    assert!(actual.is_ok());
    cost::assert_same_result(actual, reference.update(update()));
    cost::assert_same_estimator(&optimized, &reference);
    assert!(same_observation_bits(
        Some(&observation),
        Some(&observation)
    ));

    for bits in [0x7fc0_5678, 0xffc0_1234] {
        for component in 0..7 {
            let mut changed = raw;
            let grip = changed.grips[0].as_mut().unwrap();
            if component < 3 {
                grip.tracked.pose.position[component] = f32::from_bits(bits);
            } else {
                let mut orientation = grip.tracked.pose.orientation.to_array();
                orientation[component - 3] = f32::from_bits(bits);
                grip.tracked.pose.orientation = Quat::from_array(orientation);
            }
            let changed = Observation::new(changed, *original.pair()).unwrap();
            assert!(!same_observation_bits(Some(&observation), Some(&changed)));
        }
    }
    let mut changed = raw;
    changed.grips[0]
        .as_mut()
        .unwrap()
        .tracked
        .linear_velocity
        .as_mut()
        .unwrap()
        .x = 0.0;
    let changed = Observation::new(changed, *original.pair()).unwrap();
    assert!(!same_observation_bits(Some(&observation), Some(&changed)));

    assert!(same_support_bits(Some(support), Some(support)));
    let mut changed_support = support;
    changed_support.previous.x_axis.y = -0.0;
    assert!(!same_support_bits(Some(support), Some(changed_support)));
    changed_support.previous.x_axis.y = positive_nan;
    let mut other_support = changed_support;
    other_support.previous.x_axis.y = negative_nan;
    assert!(same_support_bits(
        Some(changed_support),
        Some(changed_support)
    ));
    assert!(!same_support_bits(
        Some(changed_support),
        Some(other_support)
    ));
}
