//! The player's head as the torso reads it. Pure, but for [`read`] and [`head_bone_world`], which
//! read the headpose, the camera's eye point, and the steady view's anchor.
//!
//! The published head is the view's anchor plus the HMD's travel from where it was at the
//! recenter; the camera renders from an eye point hung ahead of it
//! ([`jc3vr_view::hooks::camera::eye_point`]), which the cockpit composes the hands from. A head that
//! looks down turns the eyes about the neck, so the HMD's travel, and with it the published head,
//! drops and comes forward although nothing below the neck moved. The torso swings toward the neck
//! pivot instead, where the head bone goes, and the body IK's head target is the same point
//! ([`head_bone_world`]), so the head bone stays on the neck as the player looks down while the
//! camera follows the eyes.
//!
//! The pivot is the camera's own: its eye point plus [`EYES_TO_NECK`] turned by the head
//! ([`camera_neck_pivot`]), the point the rig reads as the player's neck. That holds whatever frame
//! the anchor, the eye arm, and the body's turn under the view are expressed in. Where the view
//! hangs from an animated head instead (a clip that poses the body, a vehicle), the camera's eyes
//! are the clip's eye bones, held out in front of the face, and the pivot is the published head
//! carried back by the head's turn ([`neck_pivot_offset`]), as it was before the steady view took
//! the camera; the two are blended by the steady view's weight.

use body_rig::body::EYES_TO_NECK;
use glam::{Mat4, Quat, Vec3};
use jc3vr_view::{
    headpose::{anchor_pair, animation_rotation, body_relative_rotation, eye_arm, query},
    hooks::camera::eye_point,
};

use crate::controllers::locomotion::anchor::status as anchor_status;

/// The player's neck pivot and eyes in Rico's model space, as the torso's solve reads them.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct ModelHead {
    /// Where the head bone goes.
    pub head: Vec3,
    /// The eyes the chest is kept clear of.
    pub eye: Vec3,
}

/// The player's neck pivot and eyes in the model space of the root at `world`, at the VR world
/// scale `scale`. The view hangs from the root at its smoothed height on the ground
/// ([`AnchorStatus::height_offset`](crate::controllers::locomotion::anchor::AnchorStatus::height_offset)),
/// so on a stair the head trails the raw root by what the smoothing hides; that is taken back out,
/// so the solve measures the head against the same root the view does.
pub(super) fn read(world: &Mat4, scale: f32) -> ModelHead {
    let anchor = anchor_status();
    let hidden = anchor.height_offset * anchor.weight;
    let hidden = Vec3::Y * if hidden.is_finite() { hidden } else { 0.0 };
    let tracking = body_relative_rotation();
    let head = model_pivot(
        world,
        query().position - hidden,
        animation_rotation(true),
        tracking,
        scale,
    );
    let published = ModelHead {
        head,
        eye: head + tracking * eye_arm(),
    };
    let weight = pivot_weight();
    if weight <= 0.0 {
        return published;
    }
    let eye = eye_point() - hidden;
    let camera = ModelHead {
        head: world
            .inverse()
            .transform_point3(camera_neck_pivot(eye, query().orientation, scale)),
        eye: world.inverse().transform_point3(eye),
    };
    ModelHead {
        head: published.head.lerp(camera.head, weight),
        eye: published.eye.lerp(camera.eye, weight),
    }
}

/// Where the head bone goes in the world, for the character at the rotation `character_rotation`
/// and the VR world scale `scale`: the published head carried back onto the neck pivot by
/// [`neck_pivot_offset`]. A look down turns the eyes about the pivot, so aiming the head bone at
/// the published head would drop it below the shoulders; the pivot does not move, and the head bone's
/// orientation, which follows the HMD, is what puts the drawn eyes back in front of it.
pub fn head_bone_world(character_rotation: Quat, scale: f32) -> Vec3 {
    let frame = character_rotation * animation_rotation(true);
    let head = frame.inverse() * query().position;
    let published = frame * head_bone_target(head, body_relative_rotation(), scale);
    let weight = pivot_weight();
    if weight <= 0.0 {
        return published;
    }
    published.lerp(
        camera_neck_pivot(eye_point(), query().orientation, scale),
        weight,
    )
}

/// The camera's neck pivot: the eye point `eye` the camera renders from plus [`EYES_TO_NECK`]
/// turned by the head's world orientation `orientation`, at the world scale `scale`. The rig reads
/// the player's neck at the same point, from the same eyes and the same turn.
pub fn camera_neck_pivot(eye: Vec3, orientation: Quat, scale: f32) -> Vec3 {
    let pivot = eye + orientation.normalize() * EYES_TO_NECK * valid_scale(scale);
    if pivot.is_finite() { pivot } else { eye }
}

/// How much of the head's target is the camera's neck pivot rather than the published head
/// carried back onto it: the steady view's weight while the headset's eyes hang from its neutral
/// head by the neck model.
fn pivot_weight() -> f32 {
    anchor_pair()
        .map(|[_, current]| current)
        .filter(|current| current.camera_stable && current.camera_weight.is_finite())
        .map_or(0.0, |current| current.camera_weight.clamp(0.0, 1.0))
}

/// [`head_bone_world`] in body axes without a synthetic animation frame. `head_rotation` is the
/// tracking rotation against those axes, at the world scale `scale`.
pub fn head_bone_target(head: Vec3, head_rotation: Quat, scale: f32) -> Vec3 {
    head + neck_pivot_offset(head_rotation, scale)
}

/// The neck pivot's place relative to the published head, model space, for a head turned
/// `head_rotation` from neutral against the body, at the world scale `scale`: what to add to the
/// published head to have the pivot. Zero for a neutral head, since the anchor already is the
/// pivot then.
pub fn neck_pivot_offset(head_rotation: Quat, scale: f32) -> Vec3 {
    let offset = (head_rotation.normalize() * EYES_TO_NECK - EYES_TO_NECK) * valid_scale(scale);
    if offset.is_finite() {
        offset
    } else {
        Vec3::ZERO
    }
}

fn valid_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn model_pivot(world: &Mat4, head: Vec3, animation: Quat, tracking: Quat, scale: f32) -> Vec3 {
    // The HMD's pivot correction belongs to the filtered animation frame, not the raw root axes.
    let (_, root, _) = world.to_scale_rotation_translation();
    world
        .inverse()
        .transform_point3(head + root * animation * neck_pivot_offset(tracking, scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_neck_pivot_travels_through_the_animation_frame_before_the_raw_root_inverse() {
        let root = Quat::from_rotation_y(0.8) * Quat::from_rotation_z(0.2);
        let world = Mat4::from_rotation_translation(root, Vec3::new(12.0, 4.0, -6.0));
        let animation = Quat::from_rotation_x(-0.7) * Quat::from_rotation_z(0.4);
        let tracking = Quat::from_rotation_x(-0.6) * Quat::from_rotation_y(0.2);
        let scale = 1.4;
        let pivot = Vec3::new(0.1, 1.55, -0.05);
        let travel = -neck_pivot_offset(tracking, scale);
        let published = world.transform_point3(pivot) + root * animation * travel;
        let actual = model_pivot(&world, published, animation, tracking, scale);
        assert!(actual.abs_diff_eq(pivot, 1e-5), "{actual:?}");
        let wrong =
            world.inverse().transform_point3(published) + neck_pivot_offset(tracking, scale);
        assert!(wrong.distance(pivot) > 0.02);
    }

    #[test]
    fn a_neutral_head_puts_the_pivot_at_the_anchor() {
        assert_eq!(neck_pivot_offset(Quat::IDENTITY, 1.0), Vec3::ZERO);
    }

    #[test]
    fn looking_down_drops_the_eyes_but_not_the_pivot() {
        // Pitching the head down by 60 degrees about the pivot swings the eyes down and forward
        // around it.
        let down = Quat::from_rotation_x(-60f32.to_radians());
        let arm = -EYES_TO_NECK;
        let eye_travel = down * arm - arm;
        assert!(eye_travel.y < -0.05, "{eye_travel}");
        // The published head, which travels with the eyes, plus the offset is the pivot, which did
        // not move.
        let pivot_travel = eye_travel + neck_pivot_offset(down, 1.0);
        assert!(pivot_travel.length() < 1e-5, "{pivot_travel}");
        // At twice the world scale, the arm is twice as long.
        let scaled = neck_pivot_offset(down, 2.0);
        assert!((scaled - neck_pivot_offset(down, 1.0) * 2.0).length() < 1e-5);
        assert_eq!(neck_pivot_offset(Quat::IDENTITY, f32::NAN), Vec3::ZERO);
    }

    /// The eyes a head turned about the neck carries round put the camera's pivot back on that
    /// neck, whatever the turn, the view's own heading, and the world scale.
    #[test]
    fn the_camera_pivot_is_the_neck_the_eyes_turn_about() {
        let neck = Vec3::new(3.0, 1.6, -2.0);
        let view = Quat::from_rotation_y(0.9);
        for turn in [
            Quat::IDENTITY,
            Quat::from_rotation_x(-1.0),
            Quat::from_rotation_y(1.2) * Quat::from_rotation_x(0.4),
        ] {
            for scale in [1.0, 1.5] {
                let orientation = view * turn;
                let eyes = neck - orientation * EYES_TO_NECK * scale;
                let pivot = camera_neck_pivot(eyes, orientation, scale);
                assert!(pivot.abs_diff_eq(neck, 1e-5), "{pivot}");
            }
        }
        let eyes = Vec3::new(0.0, 1.7, 0.0);
        assert!(
            camera_neck_pivot(eyes, Quat::IDENTITY, f32::NAN)
                .abs_diff_eq(eyes + EYES_TO_NECK, 1e-6)
        );
    }
}
