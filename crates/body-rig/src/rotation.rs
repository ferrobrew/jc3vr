//! Rotation primitives: the yaw left of a rotation, in its two meanings, a rotation's twist about
//! an axis, and a rotation taken the short way round: its angle, limited, or as a rotation vector
//! or an angular velocity.
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

/// `rotation` normalized, with the sign that makes its scalar part non-negative: the same
/// orientation, turning the short way round, at most a half turn.
pub fn shortest(rotation: Quat) -> Quat {
    let rotation = rotation.normalize();
    if rotation.w < 0.0 {
        -rotation
    } else {
        rotation
    }
}

/// The angle `rotation` turns through the short way round, radians in `0..=π`.
pub fn angle(rotation: Quat) -> f32 {
    shortest(rotation).to_axis_angle().1
}

/// The angle between the orientations `a` and `b`, the short way round, radians in `0..=π`.
pub fn angle_between(a: Quat, b: Quat) -> f32 {
    angle(a.inverse() * b)
}

/// `rotation` the short way round, limited to turn at most `max` radians about the same axis. A
/// negative `max` is zero.
pub fn clamp_angle(rotation: Quat, max: f32) -> Quat {
    let max = max.max(0.0);
    let rotation = shortest(rotation);
    let (axis, angle) = rotation.to_axis_angle();
    if angle <= max || !axis.is_finite() {
        rotation
    } else {
        Quat::from_axis_angle(axis, max)
    }
}

/// `rotation` the short way round as its axis scaled by its angle, radians, or zero for a
/// non-finite rotation.
pub fn rotation_vector(rotation: Quat) -> Vec3 {
    let vector = shortest(rotation).to_scaled_axis();
    if vector.is_finite() {
        vector
    } else {
        Vec3::ZERO
    }
}

/// The rotation that carries `from` onto `to` the short way round, as a [`rotation_vector`] in the
/// frame both are given in.
pub fn rotation_error(from: Quat, to: Quat) -> Vec3 {
    rotation_vector(to * from.inverse())
}

/// The angular velocity, radians per second about each axis, that turns `from` into `to` the short
/// way round over `dt` seconds. `None` for a step that is not positive or an orientation or a
/// turn that is not finite.
pub fn angular_velocity(from: Quat, to: Quat, dt: f32) -> Option<Vec3> {
    let velocity = (dt > 0.0 && from.is_finite() && to.is_finite())
        .then(|| shortest(to * from.inverse()).to_scaled_axis() / dt)?;
    velocity.is_finite().then_some(velocity)
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

    #[test]
    fn the_short_way_round_is_at_most_a_half_turn() {
        let long = Quat::from_rotation_x(1.5 * std::f32::consts::PI);
        let short = shortest(long);
        assert!(short.w >= 0.0);
        assert!((short.to_axis_angle().1 - 0.5 * std::f32::consts::PI).abs() < 1e-4);
        assert!((rotation_vector(long) - Vec3::NEG_X * 0.5 * std::f32::consts::PI).length() < 1e-4);
    }

    #[test]
    fn either_sign_gives_the_same_rotation_vector() {
        let q = Quat::from_rotation_z(0.3);
        assert!((rotation_vector(-q) - Vec3::Z * 0.3).length() < 1e-6);
        assert!((rotation_vector(q * 2.0) - Vec3::Z * 0.3).length() < 1e-6);
        assert_eq!(
            rotation_vector(Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0)),
            Vec3::ZERO
        );
    }

    #[test]
    fn the_error_carries_from_onto_to() {
        let from = Quat::from_rotation_y(0.4);
        let to = Quat::from_rotation_x(0.2) * from;
        assert!((rotation_error(from, to) - Vec3::X * 0.2).length() < 1e-5);
    }

    #[test]
    fn the_angular_velocity_needs_a_positive_step() {
        let from = Quat::IDENTITY;
        let to = Quat::from_rotation_y(0.1);
        assert!((angular_velocity(from, to, 0.05).unwrap() - Vec3::Y * 2.0).length() < 1e-4);
        assert_eq!(angular_velocity(from, to, 0.0), None);
        assert_eq!(angular_velocity(from, to, f32::NAN), None);
        assert_eq!(
            angular_velocity(from, Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0), 0.1),
            None
        );
    }

    #[test]
    fn the_angle_is_the_short_way_round() {
        let long = Quat::from_rotation_x(1.5 * std::f32::consts::PI);
        assert!((angle(long) - 0.5 * std::f32::consts::PI).abs() < 1e-4);
        let held = Quat::from_rotation_y(0.4);
        assert!(angle_between(held, -held) < 1e-3);
        assert!((angle_between(held, Quat::from_rotation_x(0.3) * held) - 0.3).abs() < 1e-5);
    }

    #[test]
    fn the_clamp_keeps_the_axis_and_limits_the_angle() {
        let clamped = clamp_angle(Quat::from_rotation_x(1.0), 0.5);
        assert!((angle(clamped) - 0.5).abs() < 1e-5);
        assert!(clamped.angle_between(Quat::from_rotation_x(0.5)) < 1e-5);
        let small = Quat::from_rotation_x(0.2);
        assert!(clamp_angle(small, 0.5).angle_between(small) < 1e-6);
        assert!(angle(clamp_angle(small, -1.0)) < 1e-6);
    }
}
