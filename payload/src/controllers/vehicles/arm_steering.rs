//! Arm-pose steering for the wingsuit. Pure. The parachute steers with its handles instead
//! ([`crate::controllers::vehicles::parachute`]), which share [`combine`].
//!
//! With the arms spread as wings, the difference in the hands' heights banks (the lower hand is the
//! side the turn goes to), and both hands raised or lowered from the shoulders pitches (lowered
//! dives by default). The result is a stick vector in the move stick's convention (+X right, +Y
//! forward), which the bindings write to the move half-axes the wingsuit task reads, exactly as the
//! stick does.
//!
//! Heights are measured in the cockpit frame, whose vertical is the player's real vertical: the
//! player stands or sits upright while the character flies prone, so the pose reads the same in
//! any attitude. The shoulders come from the body dimensions below the tracked head, which holds
//! seated as well as standing: the shoulders sit as far below the eyes either way.
//!
//! The arms take over once they are spread past the engaging share of the arm span, and let go only
//! once they come in past a narrower share ([`SPREAD_HYSTERESIS`]), so a wing held near the
//! threshold does not flicker in and out.

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

/// The stick vector the pose steers with, or `None` while the arms are not spread. `engaged` is
/// whether the arms steered on the last tick, which narrows the spread they need to keep steering.
pub fn steer(pose: &ArmPose, config: &ArmSteeringConfig, engaged: bool) -> Option<Vec2> {
    if pose.spread.is_nan() || pose.spread < needed_spread(config, engaged) {
        return None;
    }
    let bank = shape(pose.bank_m, config.deadzone_m, config.full_bank_m);
    let pitch = shape(-pose.height_m, config.deadzone_m, config.full_pitch_m);
    let pitch = if config.invert_pitch { -pitch } else { pitch };
    Some(Vec2::new(bank, pitch))
}

/// The spread, as a share of the arm span, the arms need to steer: the engaging spread, or the
/// narrower one while they already steer (`engaged`).
pub fn needed_spread(config: &ArmSteeringConfig, engaged: bool) -> f32 {
    if engaged {
        config.min_spread - SPREAD_HYSTERESIS
    } else {
        config.min_spread
    }
}

/// Combine the stick with the arms: their sum, each axis limited to `-1..=1`.
pub fn combine(stick: Vec2, arms: Option<Vec2>) -> Vec2 {
    (stick + arms.unwrap_or(Vec2::ZERO)).clamp(Vec2::NEG_ONE, Vec2::ONE)
}

/// How much narrower than the engaging spread the arms may come in, as a share of the arm span,
/// before they stop steering.
pub const SPREAD_HYSTERESIS: f32 = 0.1;

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
        let v = steer(&pose(0.2, -0.2, 1.6), &CONFIG, false).unwrap();
        assert!(v.x > 0.8, "{v}");
        assert!(v.y.abs() < 1e-5);
    }

    #[test]
    fn both_hands_lowered_dive_unless_inverted() {
        let v = steer(&pose(-0.2, -0.2, 1.6), &CONFIG, false).unwrap();
        assert!(v.y > 0.5 && v.x.abs() < 1e-5, "{v}");
        let inverted = ArmSteeringConfig {
            invert_pitch: true,
            ..CONFIG
        };
        assert!(steer(&pose(-0.2, -0.2, 1.6), &inverted, false).unwrap().y < 0.0);
    }

    #[test]
    fn arms_at_the_sides_steer_nothing() {
        assert_eq!(steer(&pose(0.3, -0.3, 0.4), &CONFIG, false), None);
        let nan = ArmPose {
            spread: f32::NAN,
            ..ArmPose::default()
        };
        assert_eq!(steer(&nan, &CONFIG, false), None);
    }

    #[test]
    fn spread_arms_keep_steering_until_they_come_in_past_the_hysteresis() {
        let between = CONFIG.min_spread - SPREAD_HYSTERESIS * 0.5;
        let arm_span = 1.8;
        let at = |spread: f32| pose(0.0, 0.0, spread * arm_span);
        assert_eq!(steer(&at(between), &CONFIG, false), None);
        assert!(steer(&at(between), &CONFIG, true).is_some());
        let narrow = CONFIG.min_spread - SPREAD_HYSTERESIS * 1.5;
        assert_eq!(steer(&at(narrow), &CONFIG, true), None);
    }

    #[test]
    fn seated_spread_arms_at_shoulder_height_steer_level() {
        // A seated player 1.82 m tall, from the logged body dimensions: the eyes 0.21 m above the
        // shoulders, the head at the cockpit's origin, the arm span 1.8 m, and the grips a hand in
        // from the fingertips.
        let (eye_to_shoulder, arm_span, hand) = (0.21, 1.805, 0.1);
        let shoulder_y = -eye_to_shoulder;
        let half = arm_span * 0.5 - hand;
        let level = arm_pose(
            Vec3::new(-half, shoulder_y, 0.0),
            Vec3::new(half, shoulder_y, 0.0),
            shoulder_y,
            arm_span,
        );
        assert!(level.spread > CONFIG.min_spread, "{level:?}");
        assert_eq!(steer(&level, &CONFIG, false), Some(Vec2::ZERO));
        // Elbows bent, hands out to the sides of a chair: still a wing.
        let bent = arm_pose(
            Vec3::new(-0.5, shoulder_y, -0.1),
            Vec3::new(0.5, shoulder_y, -0.1),
            shoulder_y,
            arm_span,
        );
        assert!(steer(&bent, &CONFIG, false).is_some(), "{bent:?}");
        // Both hands in the lap, a fist apart: not a wing.
        let lap = arm_pose(
            Vec3::new(-0.15, -0.6, -0.3),
            Vec3::new(0.15, -0.6, -0.3),
            shoulder_y,
            arm_span,
        );
        assert_eq!(steer(&lap, &CONFIG, false), None);
    }

    #[test]
    fn small_offsets_are_ignored_and_the_sum_is_limited() {
        assert_eq!(
            steer(&pose(0.01, -0.01, 1.6), &CONFIG, false),
            Some(Vec2::ZERO)
        );
        let sum = combine(Vec2::new(0.8, 0.0), Some(Vec2::new(0.8, -0.2)));
        assert_eq!(sum, Vec2::new(1.0, -0.2));
    }
}
