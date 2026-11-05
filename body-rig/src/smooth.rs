//! Smoothing primitives: the share of the way an exponential follow closes in a step, and a
//! critically damped spring step.
//!
//! The degenerate inputs are settled once here, so every caller agrees on them:
//!
//! - A time constant (or half-life, or smoothing time) that is not positive, NaN included, means no
//!   smoothing: the value is the target, whatever the step.
//! - Otherwise a step that is not positive, NaN included, does not move: the value and its rate
//!   are kept. An infinite step settles fully.

use std::f32::consts::LN_2;

/// The share of the way, `0..=1`, from a value to its target that an exponential follow with the
/// time constant `tau` seconds closes over `dt` seconds: `1 - exp(-dt / tau)`.
///
/// A `tau` that is not positive follows at once (one), and otherwise a `dt` that is not positive
/// does not move (zero).
pub fn follow_factor(dt: f32, tau: f32) -> f32 {
    if tau.is_nan() || tau <= 0.0 {
        return 1.0;
    }
    if dt.is_nan() || dt <= 0.0 {
        return 0.0;
    }
    1.0 - (-dt / tau).exp()
}

/// [`follow_factor`] for a follow that closes half the way every `half_life` seconds:
/// `1 - 2^(-dt / half_life)`.
pub fn half_life_factor(dt: f32, half_life: f32) -> f32 {
    follow_factor(dt, half_life / LN_2)
}

/// One step of a critically damped spring from `value`, changing at `rate` per second, toward
/// `target`, over `dt` seconds with the smoothing time `smooth_s` (about the time to cover most of
/// the way): the new value and rate.
///
/// The decay is the rational approximation of the exponential from Game Programming Gems 4
/// ("Critically damped ease-in/ease-out smoothing"), exact for a constant target and stable for any
/// step. A `smooth_s` that is not positive lands on the target at rest, and otherwise a `dt` that
/// is not positive keeps the value and the rate.
pub fn critically_damped(value: f32, rate: f32, target: f32, smooth_s: f32, dt: f32) -> (f32, f32) {
    if smooth_s.is_nan() || smooth_s <= 0.0 {
        return (target, 0.0);
    }
    if dt.is_nan() || dt <= 0.0 {
        return (value, rate);
    }
    let omega = 2.0 / smooth_s;
    let x = omega * dt;
    let decay = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = value - target;
    let temp = (rate + omega * change) * dt;
    let rate = (rate - omega * temp) * decay;
    let value = target + (change + temp) * decay;
    (value, rate)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use test_case::test_case;

    use super::*;

    #[test_case(0.0, 0.1 => 0.0; "no time does not move")]
    #[test_case(-0.1, 0.1 => 0.0; "negative time does not move")]
    #[test_case(f32::NAN, 0.1 => 0.0; "nan time does not move")]
    #[test_case(f32::INFINITY, 0.1 => 1.0; "infinite time settles")]
    #[test_case(0.1, 0.0 => 1.0; "no time constant follows at once")]
    #[test_case(0.0, 0.0 => 1.0; "no time constant follows at once in no time")]
    #[test_case(0.1, -1.0 => 1.0; "negative time constant follows at once")]
    #[test_case(0.1, f32::NAN => 1.0; "nan time constant follows at once")]
    #[test_case(0.1, f32::INFINITY => 0.0; "infinite time constant never moves")]
    fn follow_factor_edges(dt: f32, tau: f32) -> f32 {
        follow_factor(dt, tau)
    }

    #[test]
    fn follow_factor_closes_one_time_constant() {
        assert!((follow_factor(0.5, 0.5) - (1.0 - (-1.0f32).exp())).abs() < 1e-6);
    }

    #[test]
    fn half_life_factor_closes_half_in_one_half_life() {
        assert!((half_life_factor(0.2, 0.2) - 0.5).abs() < 1e-6);
        assert!((half_life_factor(0.4, 0.2) - 0.75).abs() < 1e-6);
        assert_eq!(half_life_factor(0.0, 0.2), 0.0);
        assert_eq!(half_life_factor(0.1, 0.0), 1.0);
    }

    proptest! {
        #[test]
        fn follow_factor_is_a_share(dt in -1.0f32..10.0, tau in -1.0f32..10.0) {
            let k = follow_factor(dt, tau);
            prop_assert!((0.0..=1.0).contains(&k));
        }

        #[test]
        fn follow_steps_compose(a in 0.0f32..1.0, b in 0.0f32..1.0, tau in 0.01f32..2.0) {
            // Two steps close as much as one step of their sum.
            let two = 1.0 - (1.0 - follow_factor(a, tau)) * (1.0 - follow_factor(b, tau));
            prop_assert!((two - follow_factor(a + b, tau)).abs() < 1e-5);
        }

        #[test]
        fn critically_damped_never_overshoots_from_rest(
            from in -10.0f32..10.0,
            to in -10.0f32..10.0,
            smooth_s in 0.01f32..2.0,
            dt in 0.0f32..0.1,
        ) {
            let (mut value, mut rate) = (from, 0.0);
            for _ in 0..200 {
                (value, rate) = critically_damped(value, rate, to, smooth_s, dt);
                prop_assert!(value.is_finite() && rate.is_finite());
                prop_assert!((value - to).abs() <= (from - to).abs() + 1e-4);
            }
        }
    }

    #[test]
    fn critically_damped_settles_on_the_target() {
        let (mut value, mut rate) = (0.0, 0.0);
        for _ in 0..600 {
            (value, rate) = critically_damped(value, rate, 1.0, 0.2, 1.0 / 90.0);
        }
        assert!((value - 1.0).abs() < 1e-4);
        assert!(rate.abs() < 1e-3);
    }

    #[test_case(0.0, 0.0 => (1.0, 0.0); "no smoothing lands at rest")]
    #[test_case(0.1, f32::NAN => (1.0, 0.0); "nan smoothing lands at rest")]
    #[test_case(0.0, 0.2 => (0.5, 3.0); "no time keeps the value and rate")]
    #[test_case(f32::NAN, 0.2 => (0.5, 3.0); "nan time keeps the value and rate")]
    fn critically_damped_edges(dt: f32, smooth_s: f32) -> (f32, f32) {
        critically_damped(0.5, 3.0, 1.0, smooth_s, dt)
    }
}
