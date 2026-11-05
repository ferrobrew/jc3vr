//! Rotation primitives: the yaw left of a rotation, in its two meanings, and a rotation's twist
//! about an axis.
//!
//! Axes and headings are as in [`crate::angle`].

use glam::{EulerRot, Quat, Vec3};

use crate::angle::{wrap, yaw_of};

/// `rotation` with its pitch and roll removed: the turn about +Y that takes -Z to the ground
/// projection of `rotation`'s forward. With the forward vertical, which has no projection, the
/// Euler yaw stands in.
///
/// This keeps where the forward axis points on the ground; [`twist_about_y`] keeps the rotation's
/// part about the vertical instead, and the two differ for a rolled rotation.
pub fn level(rotation: Quat) -> Quat {
    let yaw = yaw_of(rotation * Vec3::NEG_Z).unwrap_or_else(|| rotation.to_euler(EulerRot::YXZ).0);
    Quat::from_rotation_y(yaw)
}

/// The twist part of `rotation`'s swing-twist split about +Y: its rotation about the vertical alone.
/// Identity for a rotation with no twist part to speak of, such as a half turn about a horizontal
/// axis.
pub fn twist_about_y(rotation: Quat) -> Quat {
    let twist = Quat::from_xyzw(0.0, rotation.y, 0.0, rotation.w);
    if twist.length_squared() > 1e-6 {
        twist.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// The angle of `rotation`'s twist about the unit `axis`, the twist part of its swing-twist split:
/// radians in `-π..=π`, positive anticlockwise looking down the axis toward its origin, and zero
/// for a non-finite rotation. Either sign of the quaternion gives the same angle.
pub fn twist_angle(rotation: Quat, axis: Vec3) -> f32 {
    let along = Vec3::new(rotation.x, rotation.y, rotation.z).dot(axis);
    wrap(2.0 * along.atan2(rotation.w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levelling_keeps_the_heading_and_drops_pitch_and_roll() {
        let rotation = Quat::from_euler(EulerRot::YXZ, 0.7, -0.9, 0.4);
        let levelled = level(rotation);
        let forward = levelled * Vec3::NEG_Z;
        assert!(forward.y.abs() < 1e-5);
        assert!((levelled * Vec3::Y).distance(Vec3::Y) < 1e-5);
        let want = (rotation * Vec3::NEG_Z).with_y(0.0).normalize();
        assert!(forward.angle_between(want) < 1e-4);
    }

    #[test]
    fn an_upright_rotation_is_its_own_level_and_twist() {
        let rotation = Quat::from_rotation_y(-1.2);
        assert!(level(rotation).angle_between(rotation) < 1e-5);
        assert!(twist_about_y(rotation).angle_between(rotation) < 1e-5);
    }

    #[test]
    fn looking_straight_down_levels_to_the_euler_yaw() {
        let rotation =
            Quat::from_rotation_y(0.6) * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        assert!(level(rotation).angle_between(Quat::from_rotation_y(0.6)) < 1e-3);
    }

    #[test]
    fn the_twist_drops_the_swing() {
        let rotation = Quat::from_rotation_y(0.5) * Quat::from_rotation_x(0.3);
        assert!(twist_about_y(rotation).angle_between(Quat::from_rotation_y(0.5)) < 1e-5);
        assert_eq!(
            twist_about_y(Quat::from_rotation_x(std::f32::consts::PI)),
            Quat::IDENTITY
        );
    }

    #[test]
    fn the_twist_angle_is_wrapped_and_finite() {
        let axis = Vec3::Y;
        assert!((twist_angle(Quat::from_rotation_y(0.5), axis) - 0.5).abs() < 1e-5);
        assert!((twist_angle(Quat::from_rotation_y(-3.0), axis) + 3.0).abs() < 1e-4);
        assert!((twist_angle(-Quat::from_rotation_y(-3.0), axis) + 3.0).abs() < 1e-4);
        assert_eq!(twist_angle(Quat::from_rotation_x(0.7), axis), 0.0);
        let nan = Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0);
        assert_eq!(twist_angle(nan, axis), 0.0);
    }

    #[test]
    fn the_twist_angle_ignores_the_swing() {
        // A swing about an axis square to the twist axis, applied after the twist.
        let rotation = Quat::from_rotation_x(0.4) * Quat::from_rotation_z(0.9);
        assert!((twist_angle(rotation, Vec3::Z) - 0.9).abs() < 1e-5);
    }
}
