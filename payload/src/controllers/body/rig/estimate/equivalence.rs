//! Duplicate identity ignores only unavailable pose components; stored raw packets are unchanged.

use crate::controllers::{
    body::rig::observation::{Observation, RawPose},
    frame::TrackedPose,
};

pub(crate) fn same_observation(left: &Observation, right: &Observation) -> bool {
    if left.pair() != right.pair() {
        return false;
    }
    let left = left.raw();
    let right = right.raw();
    left.focused == right.focused
        && same_raw(left.head, right.head)
        && left
            .grips
            .into_iter()
            .zip(right.grips)
            .all(|(left, right)| match (left, right) {
                (None, None) => true,
                (Some(left), Some(right)) => same_raw(left, right),
                _ => false,
            })
}

fn same_raw(left: RawPose, right: RawPose) -> bool {
    left.stamp == right.stamp && same_tracked(left.tracked, right.tracked)
}

fn same_tracked(left: TrackedPose, right: TrackedPose) -> bool {
    left.position_valid == right.position_valid
        && left.orientation_valid == right.orientation_valid
        && left.position_tracked == right.position_tracked
        && left.orientation_tracked == right.orientation_tracked
        && (!left.position_valid || left.pose.position == right.pose.position)
        && (!left.orientation_valid || left.pose.orientation == right.pose.orientation)
        && left.linear_velocity == right.linear_velocity
        && left.angular_velocity == right.angular_velocity
}
