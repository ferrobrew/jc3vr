//! The shoulder model: where a body's shoulder joints are from its eyes and its torso's heading.
//!
//! The shoulders hang from the eyes by one offset, given for the right shoulder in the torso's axes
//! (+X right, +Y up, +Z back) and mirrored for the left. A player's offset comes from generic
//! proportions and the measured heights ([`eyes_to_right_shoulder`]); a character's can be measured
//! on his skeleton.

use glam::{Quat, Vec3};

use crate::{
    body::{EYES_TO_NECK, neck_pivot},
    math::side::Side,
};

/// How far the eyes sit in front of the shoulder joints, metres. A generic anthropometric figure.
pub const EYES_FORWARD_OF_SHOULDERS: f32 = 0.09;

/// The right shoulder from the eyes, in the torso's axes, for shoulders `eye_drop` below the eyes
/// and `shoulder_width` apart, [`EYES_FORWARD_OF_SHOULDERS`] behind them.
pub fn eyes_to_right_shoulder(eye_drop: f32, shoulder_width: f32) -> Vec3 {
    Vec3::new(shoulder_width * 0.5, -eye_drop, EYES_FORWARD_OF_SHOULDERS)
}

/// The shoulder joints, by [`Side::index`], for eyes at `eyes` and a torso turned by the heading
/// `torso` (a turn about +Y: the shoulders do not pitch when the head nods), with the right
/// shoulder at `eyes_to_right` from the eyes in the torso's axes.
pub fn shoulders(eyes: Vec3, torso: Quat, eyes_to_right: Vec3) -> [Vec3; 2] {
    Side::BOTH.map(|side| eyes + torso * side.mirror_from_right(eyes_to_right))
}

/// Where the eyes of a head at `eyes` turned by `head` would be with the head level and facing the
/// torso's heading `torso`, about the same neck pivot: the point the shoulders hang from, so that a
/// nod or a glance does not move them.
pub fn level_eyes(eyes: Vec3, head: Quat, torso: Quat) -> Vec3 {
    neck_pivot(eyes, head) - torso * EYES_TO_NECK
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn yaw_rotation() -> impl Strategy<Value = Quat> {
        (-3.1f32..3.1).prop_map(Quat::from_rotation_y)
    }

    proptest! {
        #[test]
        fn the_shoulders_stay_the_width_apart_and_centred_below_the_eyes(
            eyes in proptest::array::uniform3(-50.0f32..50.0),
            torso in yaw_rotation(),
            drop in 0.05f32..0.4,
            width in 0.2f32..0.6,
        ) {
            let eyes = Vec3::from(eyes);
            let [left, right] = shoulders(eyes, torso, eyes_to_right_shoulder(drop, width));
            prop_assert!((left.distance(right) - width).abs() < 1e-3);
            let middle = (left + right) * 0.5;
            let expected = eyes + torso * Vec3::new(0.0, -drop, EYES_FORWARD_OF_SHOULDERS);
            prop_assert!(middle.abs_diff_eq(expected, 1e-3));
        }

        #[test]
        fn any_nod_or_glance_leaves_the_level_eyes_in_place(
            torso in yaw_rotation(),
            nod in -1.2f32..1.2,
            glance in -1.5f32..1.5,
        ) {
            let neck = Vec3::new(0.1, 1.5, 0.2);
            let eyes_of = |head: Quat| neck - head * EYES_TO_NECK;
            let level = level_eyes(eyes_of(torso), torso, torso);
            let head = Quat::from_rotation_y(glance) * torso * Quat::from_rotation_x(nod);
            prop_assert!(level_eyes(eyes_of(head), head, torso).abs_diff_eq(level, 1e-4));
        }
    }

    #[test]
    fn the_shoulders_sit_below_behind_and_either_side_of_the_eyes() {
        let eyes = Vec3::new(0.0, 1.6, 0.0);
        let offset = eyes_to_right_shoulder(0.2, 0.4);
        let [left, right] = shoulders(eyes, Quat::IDENTITY, offset);
        assert!(left.abs_diff_eq(Vec3::new(-0.2, 1.4, EYES_FORWARD_OF_SHOULDERS), 1e-6));
        assert!(right.abs_diff_eq(Vec3::new(0.2, 1.4, EYES_FORWARD_OF_SHOULDERS), 1e-6));
        // A quarter turn left faces -X, so the right shoulder moves to -Z.
        let [_, turned] = shoulders(
            eyes,
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            offset,
        );
        assert!(turned.abs_diff_eq(Vec3::new(EYES_FORWARD_OF_SHOULDERS, 1.4, -0.2), 1e-6));
    }

    #[test]
    fn a_nod_or_a_glance_leaves_the_level_eyes_in_place() {
        let torso = Quat::from_rotation_y(0.3);
        let neck = Vec3::new(0.1, 1.5, 0.2);
        let eyes_of = |head: Quat| neck - head * EYES_TO_NECK;
        let level = level_eyes(eyes_of(torso), torso, torso);
        for head in [
            Quat::from_rotation_x(-0.7) * torso,
            Quat::from_rotation_y(0.9) * torso,
        ] {
            assert!(level_eyes(eyes_of(head), head, torso).abs_diff_eq(level, 1e-5));
        }
    }
}
