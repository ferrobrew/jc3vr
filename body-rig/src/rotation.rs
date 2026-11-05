//! Rotation primitives: the yaw left of a rotation, in its two meanings.
//!
//! Axes and headings are as in [`crate::angle`].

use glam::{EulerRot, Quat, Vec3};

use crate::angle::yaw_of;

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
}
