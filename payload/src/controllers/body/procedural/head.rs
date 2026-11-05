//! The player's head as the torso reads it. Pure, but for [`read`], which reads the headpose and
//! the steady view's anchor.
//!
//! The headpose is the eye point: the animated head anchor plus the HMD's travel from where it was
//! at the recenter. A head that looks down turns the eyes about the neck, so the eye point drops and
//! comes forward although nothing below the neck moved. The torso swings toward the neck pivot
//! instead ([`neck_pivot_offset`]): the eye point less the HMD's rotation of the arm from the pivot
//! to the eyes, which is where the head bone is. The body IK's head target is the same point
//! ([`head_bone_world`]), so the head bone stays on the neck as the player looks down while the
//! camera stays at the eye point.

use glam::{Mat4, Quat, Vec3};

use crate::{
    controllers::{
        body::rig::torso::EYES_TO_NECK_PIVOT, locomotion::anchor::status as anchor_status,
    },
    headpose::{body_relative_rotation, query},
};

/// The player's neck pivot in the model space of the root at `world`, at the VR world scale
/// `scale`. The view hangs from the root at its smoothed height on the ground
/// ([`AnchorStatus::height_offset`](crate::controllers::locomotion::anchor::AnchorStatus::height_offset)),
/// so on a stair the head trails the raw root by what the smoothing hides; that is taken back out,
/// so the solve measures the head against the same root the view does.
pub(super) fn read(world: &Mat4, scale: f32) -> Vec3 {
    let anchor = anchor_status();
    let hidden = anchor.height_offset * anchor.weight;
    let hidden = Vec3::Y * if hidden.is_finite() { hidden } else { 0.0 };
    let rotation = body_relative_rotation();
    world.inverse().transform_point3(query().position - hidden) + neck_pivot_offset(rotation, scale)
}

/// Where the head bone goes in the world, for the character at the rotation `character_rotation`
/// and the VR world scale `scale`: the eye point carried back onto the neck pivot by
/// [`neck_pivot_offset`]. A look down turns the eyes about the pivot, so aiming the head bone at
/// the eye point would drop it below the shoulders; the pivot does not move, and the head bone's
/// orientation, which follows the HMD, is what puts the drawn eyes back in front of it.
pub fn head_bone_world(character_rotation: Quat, scale: f32) -> Vec3 {
    query().position + character_rotation * neck_pivot_offset(body_relative_rotation(), scale)
}

/// [`head_bone_world`] in the character's model space: the head bone's place for the eye point
/// `eye`, with the head turned `head_rotation` against the body, at the world scale `scale`.
pub fn head_bone_target(eye: Vec3, head_rotation: Quat, scale: f32) -> Vec3 {
    eye + neck_pivot_offset(head_rotation, scale)
}

/// The neck pivot's place relative to the eye point, model space, for a head turned
/// `head_rotation` from neutral against the body, at the world scale `scale`: what to add to the
/// eye-derived head to have the pivot. Zero for a neutral head, since the anchor already is the
/// pivot then.
pub fn neck_pivot_offset(head_rotation: Quat, scale: f32) -> Vec3 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let offset = (head_rotation.normalize() * EYES_TO_NECK_PIVOT - EYES_TO_NECK_PIVOT) * scale;
    if offset.is_finite() {
        offset
    } else {
        Vec3::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_neutral_head_puts_the_pivot_at_the_anchor() {
        assert_eq!(neck_pivot_offset(Quat::IDENTITY, 1.0), Vec3::ZERO);
    }

    #[test]
    fn looking_down_drops_the_eyes_but_not_the_pivot() {
        // Pitching the head down by 60 degrees about the pivot swings the eyes down and forward
        // around it.
        let down = Quat::from_rotation_x(-60f32.to_radians());
        let arm = -EYES_TO_NECK_PIVOT;
        let eye_travel = down * arm - arm;
        assert!(eye_travel.y < -0.05, "{eye_travel}");
        // The eye-derived head plus the offset is the pivot, which did not move.
        let pivot_travel = eye_travel + neck_pivot_offset(down, 1.0);
        assert!(pivot_travel.length() < 1e-5, "{pivot_travel}");
        // At twice the world scale, the arm is twice as long.
        let scaled = neck_pivot_offset(down, 2.0);
        assert!((scaled - neck_pivot_offset(down, 1.0) * 2.0).length() < 1e-5);
        assert_eq!(neck_pivot_offset(Quat::IDENTITY, f32::NAN), Vec3::ZERO);
    }
}
