//! Arm-pose steering for the wingsuit and the parachute. Pure.
//!
//! With the arms spread as wings, the difference in the hands' heights banks (the lower hand is the
//! side the turn goes to), and both hands raised or lowered from the shoulders pitches (lowered
//! dives by default). The result is a stick vector in the move stick's convention (+X right, +Y
//! forward), which the bindings write to the move half-axes both flight tasks read, exactly as the
//! stick does.
//!
//! Heights are measured in the cockpit frame, whose vertical is the player's real vertical: the
//! player stands or sits upright while the character flies prone, so the pose reads the same in
//! any attitude. The shoulders come from the body dimensions below the head.

use glam::{Vec2, Vec3};

use crate::controllers::vehicles::config::ArmSteeringConfig;

/// The arm pose the steering reads.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ArmPose {
    /// The left hand's height above the right's, metres: positive banks right.
    pub bank_m: f32,
    /// The hands' mean height above the shoulders, metres.
    pub height_m: f32,
    /// The hands' horizontal separation as a share of the arm span.
    pub spread: f32,
}

/// Measure the pose of hands at `left` and `right` (cockpit frame) with the shoulders at height
/// `shoulder_y` and an arm span of `arm_span` metres.
pub fn arm_pose(left: Vec3, right: Vec3, shoulder_y: f32, arm_span: f32) -> ArmPose {
    let across = Vec2::new(right.x - left.x, right.z - left.z).length();
    ArmPose {
        bank_m: left.y - right.y,
        height_m: (left.y + right.y) * 0.5 - shoulder_y,
        spread: across / arm_span.max(0.1),
    }
}

/// The stick vector the pose steers with, or `None` while the arms are not spread.
pub fn steer(pose: &ArmPose, config: &ArmSteeringConfig) -> Option<Vec2> {
    if pose.spread.is_nan() || pose.spread < config.min_spread {
        return None;
    }
    let bank = shape(pose.bank_m, config.deadzone_m, config.full_bank_m);
    let pitch = shape(-pose.height_m, config.deadzone_m, config.full_pitch_m);
    let pitch = if config.invert_pitch { -pitch } else { pitch };
    Some(Vec2::new(bank, pitch))
}

/// Combine the stick with the arms: their sum, each axis limited to `-1..=1`.
pub fn combine(stick: Vec2, arms: Option<Vec2>) -> Vec2 {
    (stick + arms.unwrap_or(Vec2::ZERO)).clamp(Vec2::NEG_ONE, Vec2::ONE)
}

/// A signed offset over a deadzone, reaching one at `full`.
fn shape(value: f32, deadzone: f32, full: f32) -> f32 {
    let magnitude = value.abs();
    if !magnitude.is_finite() || magnitude <= deadzone {
        return 0.0;
    }
    let span = (full - deadzone).max(1e-3);
    value.signum() * ((magnitude - deadzone) / span).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: ArmSteeringConfig = ArmSteeringConfig::new();

    fn pose(left_y: f32, right_y: f32, spread: f32) -> ArmPose {
        arm_pose(
            Vec3::new(-spread * 0.5, left_y, 0.0),
            Vec3::new(spread * 0.5, right_y, 0.0),
            0.0,
            1.8,
        )
    }

    #[test]
    fn a_lowered_right_hand_banks_right() {
        let v = steer(&pose(0.2, -0.2, 1.6), &CONFIG).unwrap();
        assert!(v.x > 0.8, "{v}");
        assert!(v.y.abs() < 1e-5);
    }

    #[test]
    fn both_hands_lowered_dive_unless_inverted() {
        let v = steer(&pose(-0.2, -0.2, 1.6), &CONFIG).unwrap();
        assert!(v.y > 0.5 && v.x.abs() < 1e-5, "{v}");
        let inverted = ArmSteeringConfig {
            invert_pitch: true,
            ..CONFIG
        };
        assert!(steer(&pose(-0.2, -0.2, 1.6), &inverted).unwrap().y < 0.0);
    }

    #[test]
    fn arms_at_the_sides_steer_nothing() {
        assert_eq!(steer(&pose(0.3, -0.3, 0.4), &CONFIG), None);
        let nan = ArmPose {
            spread: f32::NAN,
            ..ArmPose::default()
        };
        assert_eq!(steer(&nan, &CONFIG), None);
    }

    #[test]
    fn small_offsets_are_ignored_and_the_sum_is_limited() {
        assert_eq!(steer(&pose(0.01, -0.01, 1.6), &CONFIG), Some(Vec2::ZERO));
        let sum = combine(Vec2::new(0.8, 0.0), Some(Vec2::new(0.8, -0.2)));
        assert_eq!(sum, Vec2::new(1.0, -0.2));
    }
}
