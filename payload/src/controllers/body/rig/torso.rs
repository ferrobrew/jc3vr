//! Cockpit-space torso readouts and shared heading and neck geometry. The grounded rig supplies the
//! live torso ([`crate::controllers::body::rig::grounded::torso`]).
//!
//! Positions use +X right, +Y up, and -Z forward, in metres. A heading is a yaw about +Y,
//! positive turning -Z toward -X, as in [`crate::headpose::sim::body_yaw_of`].

use glam::{Quat, Vec3};

/// The estimated torso, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TorsoEstimate {
    /// The torso's heading at the chest, radians.
    pub yaw: f32,
    /// The head's heading, radians.
    pub head_yaw: f32,
    /// How far the head is turned from the chest, radians, positive turning left.
    pub neck_twist: f32,
    /// The hands' own heading, when they say anything.
    pub hands_yaw: Option<f32>,
    /// How much the hands counted against the head's weight: the sum of their horizontal shares
    /// cubed, `0..=2`.
    pub hands_weight: f32,
    /// The neck pivot at the base of the skull.
    pub neck: Vec3,
    /// The shoulder joints, left then right.
    pub shoulders: [Vec3; 2],
    /// The middle of the shoulder joints.
    pub chest: Vec3,
    /// The middle of the hip joints.
    pub pelvis: Vec3,
    /// The hip joints, left then right.
    pub hips: [Vec3; 2],
}

impl TorsoEstimate {
    /// The rotation about +Y by the torso's heading.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw)
    }
}

/// The distance between the hip joints as a fraction of the distance between the shoulder joints.
/// A generic anthropometric figure.
pub const HIP_SPACING_FRACTION: f32 = 0.45;

/// The furthest the chest is turned from the character's facing, degrees.
pub const CHEST_LIMIT_DEG: f32 = 45.0;

#[cfg(test)]
mod tests {
    use body_rig::angle::{direction, head_heading, wrap, yaw_of};

    use super::*;

    fn deg(d: f32) -> f32 {
        d.to_radians()
    }

    fn looking(yaw: f32, pitch: f32) -> Quat {
        Quat::from_euler(glam::EulerRot::YXZ, yaw, pitch, 0.0)
    }

    #[test]
    fn the_heading_conventions_agree() {
        for yaw in [0.0, 0.7, -2.0, 3.0] {
            assert!((yaw_of(direction(yaw)).unwrap() - yaw).abs() < 1e-5);
            // The grounded rig's headings are the same.
            assert!((head_heading(looking(yaw, 0.0)).unwrap() - yaw).abs() < 1e-5);
        }
        // Turning -Z toward -X is positive, as the body yaw is.
        assert!(yaw_of(Vec3::NEG_X).unwrap() > 0.0);
        assert!((crate::headpose::sim::body_yaw_of(Quat::from_rotation_y(0.4)) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn the_head_keeps_its_heading_looking_straight_down_or_up() {
        for yaw in [0.0, 1.0, -2.5] {
            for pitch in [deg(-89.9), deg(-60.0), deg(60.0), deg(89.9)] {
                let heading = head_heading(looking(yaw, pitch)).unwrap();
                assert!(wrap(heading - yaw).abs() < 1e-3, "{yaw} {pitch}: {heading}");
            }
        }
    }
}
