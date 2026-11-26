//! Axis shaping: how far an input travels past its deadzone, rescaled so that full travel still
//! reaches one, and bent by a response exponent. A stick, a trigger, a wheel's or a handlebar's
//! turn, a yoke, and an arm's lean all read their input this way.
//!
//! The degenerate inputs are settled here: a non-finite input reads as zero, a deadzone that is
//! negative or NaN is none, a full travel no further than the deadzone is a step at the deadzone's
//! edge, and a response that is not finite is linear. Each caller still bounds its own deadzone to
//! what its input can use (a stick's to less than its full deflection, a wheel's to half its lock).

use glam::Vec2;

/// The signed share, `-1..=1`, of `value`'s travel from the edge of `deadzone` to `full`: zero
/// within the deadzone, one from `full` on, with `value`'s sign. Non-finite `value` reads as zero.
pub fn past_deadzone(value: f32, deadzone: f32, full: f32) -> f32 {
    let deadzone = deadzone.max(0.0);
    let magnitude = value.abs();
    if !magnitude.is_finite() || magnitude <= deadzone {
        return 0.0;
    }
    value.signum() * ((magnitude - deadzone) / (full - deadzone).max(MIN_SPAN)).min(1.0)
}

/// [`past_deadzone`] bent by the exponent `response`: above one the output starts slowly and
/// steepens, below one it starts quickly. The exponent is held to [`MIN_RESPONSE`] and
/// [`MAX_RESPONSE`], and a non-finite one is linear.
pub fn shaped(value: f32, deadzone: f32, full: f32, response: f32) -> f32 {
    let share = past_deadzone(value, deadzone, full);
    share.signum() * share.abs().powf(response_exponent(response))
}

/// The input that [`shaped`] turns into `output`: its inverse over `-1..=1`, with an output past
/// one read as one, and zero for zero or a non-finite output.
pub fn unshaped(output: f32, deadzone: f32, full: f32, response: f32) -> f32 {
    if !output.is_finite() || output == 0.0 {
        return 0.0;
    }
    let deadzone = deadzone.max(0.0);
    let share = output
        .abs()
        .min(1.0)
        .powf(1.0 / response_exponent(response));
    output.signum() * (deadzone + share * (full - deadzone).max(0.0))
}

/// A two-axis input (a stick) through a radial deadzone: zero within `deadzone` of the centre, and
/// outside it the direction kept and the length rescaled by [`past_deadzone`], reaching one at
/// `full`.
pub fn radial_past_deadzone(stick: Vec2, deadzone: f32, full: f32) -> Vec2 {
    let length = stick.length();
    let share = past_deadzone(length, deadzone, full);
    if share == 0.0 {
        Vec2::ZERO
    } else {
        stick * (share / length)
    }
}

/// The gentlest response exponent [`shaped`] uses.
pub const MIN_RESPONSE: f32 = 0.2;

/// The steepest response exponent [`shaped`] uses.
pub const MAX_RESPONSE: f32 = 5.0;

/// The shortest travel from the deadzone's edge to full that the share divides by, so that a full
/// travel at the deadzone is a step rather than a division by zero.
const MIN_SPAN: f32 = 1e-3;

fn response_exponent(response: f32) -> f32 {
    if response.is_finite() {
        response.clamp(MIN_RESPONSE, MAX_RESPONSE)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use test_case::test_case;

    use super::*;

    #[test_case(0.05 => 0.0; "inside the deadzone")]
    #[test_case(0.1 => 0.0; "at the deadzone's edge")]
    #[test_case(0.55 => 0.5; "half way")]
    #[test_case(-0.55 => -0.5; "half way back")]
    #[test_case(1.0 => 1.0; "full")]
    #[test_case(3.0 => 1.0; "past full")]
    #[test_case(f32::NAN => 0.0; "nan")]
    #[test_case(f32::INFINITY => 0.0; "infinite")]
    fn past_a_deadzone_of_a_tenth(value: f32) -> f32 {
        let share = past_deadzone(value, 0.1, 1.0);
        (share * 1e4).round() / 1e4
    }

    #[test]
    fn a_degenerate_deadzone_or_travel_is_settled() {
        assert_eq!(past_deadzone(0.5, f32::NAN, 1.0), 0.5);
        assert_eq!(past_deadzone(0.5, -1.0, 1.0), 0.5);
        assert_eq!(past_deadzone(0.31, 0.3, 0.3), 1.0);
        assert_eq!(shaped(0.5, 0.0, 1.0, f32::NAN), 0.5);
        assert!((shaped(0.5, 0.0, 1.0, 100.0) - 0.5f32.powf(MAX_RESPONSE)).abs() < 1e-7);
    }

    #[test]
    fn a_stick_keeps_its_direction() {
        let stick = Vec2::new(0.3, 0.4);
        let out = radial_past_deadzone(stick, 0.25, 1.0);
        assert!((out.length() - (0.25 / 0.75)).abs() < 1e-6);
        assert!(out.normalize().abs_diff_eq(stick.normalize(), 1e-6));
        assert_eq!(
            radial_past_deadzone(Vec2::new(0.1, 0.1), 0.25, 1.0),
            Vec2::ZERO
        );
        assert_eq!(
            radial_past_deadzone(Vec2::new(f32::NAN, 0.0), 0.25, 1.0),
            Vec2::ZERO
        );
        assert!((radial_past_deadzone(Vec2::new(0.0, 2.0), 0.25, 1.0).y - 1.0).abs() < 1e-6);
    }

    proptest! {
        #[test]
        fn the_share_is_odd_bounded_and_monotonic(
            a in -3.0f32..3.0,
            b in -3.0f32..3.0,
            deadzone in 0.0f32..0.9,
            response in 0.2f32..5.0,
        ) {
            let (low, high) = if a <= b { (a, b) } else { (b, a) };
            let shape = |v| shaped(v, deadzone, 1.0, response);
            prop_assert!((-1.0..=1.0).contains(&shape(a)));
            prop_assert_eq!(shape(-a), -shape(a));
            prop_assert!(shape(low) <= shape(high));
        }

        #[test]
        fn unshaped_inverts_shaped_past_the_deadzone(
            value in 0.0f32..1.0,
            deadzone in 0.0f32..0.5,
            full in 0.6f32..2.0,
            response in 0.5f32..3.0,
            negative: bool,
        ) {
            let value = deadzone + value * (full - deadzone);
            let value = if negative { -value } else { value };
            let output = shaped(value, deadzone, full, response);
            prop_assume!(output != 0.0);
            prop_assert!((unshaped(output, deadzone, full, response) - value).abs() < 1e-3);
        }
    }
}
