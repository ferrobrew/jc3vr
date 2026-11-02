//! The procedural body's solver: Rico's torso posed from the player's head on the hips the
//! animation keeps. Pure, and in Rico's model space (+Y up, facing -Z, the left side at -X, the
//! root at the feet).
//!
//! - [`anchored`]: the chest's twist toward the player's torso, the spine's swing toward the neck
//!   under the player's head, the chest's pitch for a look down, and the neck's share of the head's
//!   turn, all from the clip's own spine;
//! - [`clavicles`]: the shoulders reaching with the hands;
//! - [`arms`]: the torso the wrist targets are kept out of, and the elbow hints;
//! - [`spine`]: the swing and the shares the spine's bones take;
//! - [`twist`]: the limbs' twist bones;
//! - [`wings`]: Rico's arms in the wingsuit;
//! - [`pose`]: the rig's bones in model space.
//!
//! Everything is a rotation composed onto the animated bones, so no bone's rest frame needs to be
//! known, and the hips, the pelvis, and the legs stay the animation's.

pub mod anchored;
pub mod arms;
pub mod clavicles;
pub mod pose;
pub mod spine;
pub mod twist;
pub mod wings;

use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::procedural::solver::pose::RigPose;

/// One solve's inputs, model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SolveInput {
    /// The animated pose.
    pub animated: RigPose,
    /// Where the head bone should be: the player's head, at the neck pivot.
    pub head: Vec3,
    /// The player's head rotation relative to the body (identity faces forward).
    pub head_rotation: Quat,
    /// The player's torso heading relative to Rico's facing, radians about +Y, when estimated.
    pub torso_yaw: Option<f32>,
}

/// The yaw of the horizontal line from `left` to `right` about +Y, measured from +X: the facing of
/// a pair of bones on either side of the body.
pub fn line_yaw(left: Vec3, right: Vec3) -> Option<f32> {
    let line = Vec2::new(right.x - left.x, right.z - left.z);
    (line.length_squared() > 1e-8).then(|| (-line.y).atan2(line.x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_from_left_to_right_faces_forward() {
        assert!(line_yaw(Vec3::new(-1.0, 0.0, 0.0), Vec3::X).unwrap().abs() < 1e-6);
        assert_eq!(line_yaw(Vec3::ZERO, Vec3::Y), None);
    }
}
