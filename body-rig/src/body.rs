//! The player's body as the rig measures it: the neck pivot under the eyes, and the waist pivot's
//! calibrated height.

use glam::{Quat, Vec3};

/// The neck pivot, at the base of the skull, from the eyes in the head's frame: below and behind
/// them, so that nodding and turning the head swing the eyes about it and leave it in place. A
/// generic anthropometric figure.
pub const EYES_TO_NECK: Vec3 = Vec3::new(0.0, -0.075, 0.085);

/// The waist pivot's height as a share of the standing height: about where the lumbar spine meets
/// the pelvis, which is where a bend from the hips turns. A generic anthropometric figure.
pub const HIP_HEIGHT_SHARE: f32 = 0.53;

/// The shortest spine the rig works with, metres, so that a garbled calibration cannot divide the
/// arc by nothing.
pub const MIN_SPINE_M: f32 = 0.3;

/// The player's calibrated heights, metres over the floor.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PlayerBody {
    /// The standing eye height.
    pub eye_height: f32,
    /// The waist pivot's standing height.
    pub hip_height: f32,
}

impl PlayerBody {
    /// The body of a player `height` tall with the eyes at `eye_height`.
    pub fn from_heights(height: f32, eye_height: f32) -> Self {
        Self {
            eye_height,
            hip_height: height * HIP_HEIGHT_SHARE,
        }
    }

    /// The neck pivot's height for an upright head at the standing eye height.
    pub fn standing_neck(&self) -> f32 {
        self.eye_height + EYES_TO_NECK.y
    }

    /// The spine's length, waist pivot to neck pivot, for a neck that stands at `standing_neck`.
    pub fn spine_length(&self, standing_neck: f32) -> f32 {
        let length = standing_neck - self.hip_height;
        if length.is_finite() {
            length.max(MIN_SPINE_M)
        } else {
            MIN_SPINE_M
        }
    }
}

/// The neck pivot for eyes at `eyes` turned by `head`.
pub fn neck_pivot(eyes: Vec3, head: Quat) -> Vec3 {
    eyes + head.normalize() * EYES_TO_NECK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodding_leaves_the_neck_in_place() {
        let eyes = Vec3::new(0.0, 1.7, 0.0);
        let level = neck_pivot(eyes, Quat::IDENTITY);
        let nod = Quat::from_rotation_x(-0.6);
        // The eyes swing about the pivot: carry them as a nod would.
        let nodded_eyes = level - nod * EYES_TO_NECK;
        assert!((neck_pivot(nodded_eyes, nod) - level).length() < 1e-5);
    }

    #[test]
    fn the_spine_is_never_shorter_than_the_floor() {
        let body = PlayerBody::from_heights(1.8, 1.68);
        assert!((body.spine_length(body.standing_neck()) - (1.605 - 0.954)).abs() < 1e-4);
        assert_eq!(body.spine_length(0.5), MIN_SPINE_M);
        assert_eq!(body.spine_length(f32::NAN), MIN_SPINE_M);
    }
}
