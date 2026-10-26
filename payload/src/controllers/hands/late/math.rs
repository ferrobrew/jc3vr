//! The pure pose and transform math the late update builds its correction from, independent of any
//! frame state.

use glam::Mat4;

use crate::controllers::{frame::Pose, hands::pose as hand_pose};

/// Derive a bounded correction from the physical move between the reference and current wrist.
///
/// The relative position and rotation are validated before the correction is weighted. This keeps
/// a stationary wrist rotation valid even when the world origin is far away, and rejects
/// discontinuities before they can be interpolated into a plausible-looking correction.
pub(crate) fn correction(now: Pose, reference: Pose, weight: f32) -> Option<Pose> {
    (weight.is_finite() && plausible(now, reference))
        .then(|| hand_pose::weighted_delta(now, reference, weight))
}

/// Whether the wrist moved by a tick or two rather than discontinuously (a teleport, a recenter, or
/// a respawn), which must not be smeared onto the rendered hand.
pub(crate) fn plausible(now: Pose, reference: Pose) -> bool {
    relative_metrics(now, reference).is_some_and(|(displacement, rotation)| {
        displacement < MAX_SHIFT_M && rotation < MAX_TURN_RAD.to_degrees()
    })
}

/// The engine's transform interpolation (`CTransform::Lerp`): the translation and scale lerped and
/// the rotation slerped.
pub(crate) fn lerp_transform(a: Mat4, b: Mat4, t: f32) -> Mat4 {
    let (sa, ra, ta) = a.to_scale_rotation_translation();
    let (sb, rb, tb) = b.to_scale_rotation_translation();
    Mat4::from_scale_rotation_translation(sa.lerp(sb, t), ra.slerp(rb, t), ta.lerp(tb, t))
}

/// Interpolate an attachment in its parent's frame, preserving its local mechanical motion.
/// World-space interpolation cuts across the turn of a moving parent instead of following it.
pub(crate) fn interpolate_attached(parent: [Mat4; 2], attachment: [Mat4; 2], t: f32) -> Mat4 {
    let local = [
        parent[0].inverse() * attachment[0],
        parent[1].inverse() * attachment[1],
    ];
    if local.iter().any(|m| !m.is_finite()) {
        return lerp_transform(attachment[0], attachment[1], t);
    }
    lerp_transform(parent[0], parent[1], t) * lerp_transform(local[0], local[1], t)
}

pub(crate) fn pose_matrix(p: Pose) -> Mat4 {
    Mat4::from_rotation_translation(p.orientation, p.position)
}

/// The physical wrist displacement and relative rotation, or `None` for a non-finite pose.
pub(super) fn relative_metrics(now: Pose, reference: Pose) -> Option<(f32, f32)> {
    (now.position.is_finite()
        && reference.position.is_finite()
        && now.orientation.is_finite()
        && reference.orientation.is_finite())
    .then(|| {
        let displacement = (now.position - reference.position).length();
        let mut relative = hand_pose::delta(now, reference).orientation;
        if relative.w < 0.0 {
            relative = -relative;
        }
        let (_, angle) = relative.to_axis_angle();
        (displacement, angle.to_degrees())
    })
    .filter(|(displacement, rotation)| displacement.is_finite() && rotation.is_finite())
}

pub(super) const MAX_SHIFT_M: f32 = 0.5;
pub(super) const MAX_TURN_RAD: f32 = std::f32::consts::FRAC_PI_2;

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec3};

    use super::*;

    #[test]
    fn a_stationary_rotating_wrist_is_corrected_at_a_translated_origin() {
        let reference = Pose {
            position: Vec3::new(1000.0, 200.0, -300.0),
            orientation: Quat::IDENTITY,
        };
        let now = Pose {
            orientation: Quat::from_rotation_y(0.4),
            ..reference
        };
        let d = correction(now, reference, 0.5).unwrap();
        let carried = hand_pose::mul(d, reference);
        assert!(carried.position.distance(reference.position) < 1e-5);
        assert!(
            (carried
                .orientation
                .angle_between(Quat::from_rotation_y(0.2)))
            .abs()
                < 1e-5
        );
    }

    #[test]
    fn invalid_or_discontinuous_motion_is_not_corrected() {
        let reference = Pose {
            position: Vec3::new(0.3, 1.0, 0.0),
            orientation: Quat::IDENTITY,
        };
        assert!(
            correction(
                Pose {
                    orientation: Quat::from_rotation_y(std::f32::consts::PI),
                    ..reference
                },
                reference,
                1.0,
            )
            .is_none()
        );
        assert!(correction(reference, reference, f32::NAN).is_none());
    }

    #[test]
    fn a_rigid_attachment_follows_platform_translation_and_rotation() {
        let local = Mat4::from_translation(Vec3::new(0.4, 1.2, -0.5));
        for (distance, yaw) in [(2.0, 0.0), (0.0, 1.0), (2.0, -0.6)] {
            let parent = [
                Mat4::IDENTITY,
                Mat4::from_rotation_translation(Quat::from_rotation_y(yaw), Vec3::X * distance),
            ];
            let attachment = [parent[0] * local, parent[1] * local];
            for t in [0.0, 0.25, 0.5, 1.0] {
                let expected = lerp_transform(parent[0], parent[1], t) * local;
                assert!(interpolate_attached(parent, attachment, t).abs_diff_eq(expected, 1e-5));
            }
        }
    }

    #[test]
    fn the_weapon_interpolation_matches_the_endpoints() {
        let a = Mat4::from_rotation_translation(Quat::from_rotation_x(0.2), Vec3::X);
        let b = Mat4::from_rotation_translation(Quat::from_rotation_x(0.6), Vec3::Y);
        assert!(lerp_transform(a, b, 0.0).abs_diff_eq(a, 1e-5));
        assert!(lerp_transform(a, b, 1.0).abs_diff_eq(b, 1e-5));
    }
}
