//! Headings and their arithmetic.
//!
//! Positions use +X right, +Y up, and -Z forward, in metres. A heading is a yaw about +Y in
//! radians, zero facing -Z, positive turning -Z toward -X, as the payload's body yaw reads it.

use glam::{Quat, Vec2, Vec3};

/// `angle` wrapped to `-π..=π`. A non-finite angle wraps to zero.
pub fn wrap(angle: f32) -> f32 {
    if !angle.is_finite() {
        return 0.0;
    }
    let tau = std::f32::consts::TAU;
    let wrapped = (angle + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    if wrapped <= -std::f32::consts::PI {
        wrapped + tau
    } else {
        wrapped
    }
}

/// The heading of a direction's horizontal part, or `None` for a direction with no horizontal
/// extent.
pub fn yaw_of(direction: Vec3) -> Option<f32> {
    yaw_of_flat(Vec2::new(direction.x, direction.z))
}

/// The heading of a horizontal `(x, z)` direction, or `None` for a zero or non-finite one.
pub fn yaw_of_flat(flat: Vec2) -> Option<f32> {
    (flat.length_squared() > 1.0e-10 && flat.is_finite()).then(|| (-flat.x).atan2(-flat.y))
}

/// The horizontal `(x, z)` forward of a heading.
pub fn forward(yaw: f32) -> Vec2 {
    let v = Quat::from_rotation_y(yaw) * Vec3::NEG_Z;
    Vec2::new(v.x, v.z)
}

/// The horizontal `(x, z)` right of a heading.
pub fn right(yaw: f32) -> Vec2 {
    let f = forward(yaw);
    Vec2::new(-f.y, f.x)
}

/// The head's heading: its forward axis on the ground, blended with its up axis as it pitches, so
/// that the heading stays defined looking straight down or up. Looking down, the up axis points
/// the way the face does; looking up, it points back.
pub fn head_heading(orientation: Quat) -> Option<f32> {
    let forward = orientation * Vec3::NEG_Z;
    let up = orientation * Vec3::Y;
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    yaw_of_flat(flat(forward) - flat(up) * forward.y)
}

/// How far the head's forward axis is pitched from level, radians, positive looking up.
pub fn head_pitch(orientation: Quat) -> f32 {
    let forward = orientation * Vec3::NEG_Z;
    let pitch = forward.y.clamp(-1.0, 1.0).asin();
    if pitch.is_finite() { pitch } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use test_case::test_case;

    use super::*;

    #[test_case(0.0, 0.0; "zero")]
    #[test_case(std::f32::consts::PI, std::f32::consts::PI; "half turn stays positive")]
    #[test_case(-std::f32::consts::PI, std::f32::consts::PI; "negative half turn flips")]
    #[test_case(3.0 * std::f32::consts::PI, std::f32::consts::PI; "one and a half turns")]
    #[test_case(f32::NAN, 0.0; "nan")]
    fn wraps(angle: f32, expected: f32) {
        assert!((wrap(angle) - expected).abs() < 1e-5);
    }

    #[test]
    fn forward_and_yaw_agree() {
        assert!((forward(0.0) - Vec2::new(0.0, -1.0)).length() < 1e-6);
        // A quarter turn left faces -X.
        let left = std::f32::consts::FRAC_PI_2;
        assert!((forward(left) - Vec2::new(-1.0, 0.0)).length() < 1e-6);
        assert!((right(0.0) - Vec2::new(1.0, 0.0)).length() < 1e-6);
        assert_eq!(yaw_of(Vec3::Y), None);
    }

    #[test]
    fn looking_down_keeps_the_heading() {
        let yaw = 0.7;
        let down = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-1.5);
        assert!((head_heading(down).unwrap() - yaw).abs() < 1e-4);
        assert!(head_pitch(down) < -1.4);
    }

    proptest! {
        #[test]
        fn wrap_stays_in_range(angle in -100.0f32..100.0) {
            let w = wrap(angle);
            prop_assert!(w > -std::f32::consts::PI - 1e-5 && w <= std::f32::consts::PI + 1e-5);
            prop_assert!((w.sin() - angle.sin()).abs() < 1e-3);
        }

        #[test]
        fn yaw_of_inverts_forward(yaw in -3.1f32..3.1) {
            let back = yaw_of_flat(forward(yaw)).unwrap();
            prop_assert!(wrap(back - yaw).abs() < 1e-4);
        }
    }
}
