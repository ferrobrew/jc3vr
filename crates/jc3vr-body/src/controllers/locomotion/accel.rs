//! The stick locomotion's pure core: the stick's target velocity in the world and the fixed-rate
//! integrator that moves the body's velocity toward it. Velocities are world `(x, z)` in metres per
//! second.
//!
//! The integrator is a vector clamp: each tick the velocity moves straight toward the target by at
//! most the rate times the step. A fixed rate is what makes the motion predictable: a start, a
//! stop, and a change of direction all take a time proportional to the change, and nothing
//! overshoots, so there is no surge for the view to feel.

use glam::Vec2;

use crate::controllers::locomotion::config::{SpeedSource, StickConfig};

/// The rates of the integrator, in metres per second squared.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Rates {
    /// While the target is at least as fast as the velocity and not behind it.
    pub accelerate: f32,
    /// While the target is slower than the velocity or turned away from it.
    pub decelerate: f32,
}

impl Rates {
    /// The integrator's fixed rates. The deceleration is higher than the acceleration, so stopping
    /// is crisp.
    pub const FIXED: Self = Self {
        accelerate: 10.0,
        decelerate: 14.0,
    };
}

/// Move `velocity` toward `target` by at most the applicable rate times `dt`. Non-finite inputs
/// leave the velocity as it was, or stop it if it is itself not finite.
pub fn step(velocity: Vec2, target: Vec2, dt: f32, rates: Rates) -> Vec2 {
    if !velocity.is_finite() {
        return Vec2::ZERO;
    }
    if !target.is_finite() || !dt.is_finite() || dt <= 0.0 {
        return velocity;
    }
    let speeding_up =
        target.length_squared() >= velocity.length_squared() && target.dot(velocity) >= 0.0;
    let rate = if speeding_up {
        rates.accelerate
    } else {
        rates.decelerate
    };
    let max_change = rate.max(0.0) * dt;
    let change = target - velocity;
    let length = change.length();
    if length <= max_change {
        target
    } else {
        velocity + change * (max_change / length)
    }
}

/// The velocity to step from, given the body's own horizontal velocity `body`. A velocity faster
/// than `target` coasts down from speed the stick did not give it (a landing, the end of a reel),
/// and a coast never runs ahead of the body: whatever slowed the body (the ground, a slope, a wall)
/// slows the coast with it, so the integrator cannot hold a speed the body lost and hand it back
/// once nothing holds the body back. A velocity no faster than the target is the stick's own and
/// is kept, as it is without a finite `body`.
pub fn coast_limited(velocity: Vec2, target: Vec2, body: Option<Vec2>) -> Vec2 {
    let Some(body) = body.filter(|b| b.is_finite()) else {
        return velocity;
    };
    let target_speed = if target.is_finite() {
        target.length()
    } else {
        0.0
    };
    velocity.clamp_length_max(body.length().max(target_speed))
}

/// The stick's direction in the world: `stick` is `(right, forward)` in the frame whose forward is
/// the unit world `(x, z)` vector `forward`, and the result is the same vector in world `(x, z)`.
/// The engine's world is right-handed with +Y up, so the right of `forward` is `(-forward.z,
/// forward.x)` written in `(x, z)`.
pub fn to_world(stick: Vec2, forward: Vec2) -> Vec2 {
    let right = Vec2::new(-forward.y, forward.x);
    right * stick.x + forward * stick.y
}

/// The target velocity for one tick. `stick` is the move stick, `(right, forward)` with a length of
/// at most one; `forward` is the direction frame's unit forward in world `(x, z)`; `game_speed` is
/// the game's target speed for the stick, when it has one.
pub fn target(stick: Vec2, forward: Vec2, game_speed: Option<f32>, config: &StickConfig) -> Vec2 {
    let magnitude = stick.length().min(1.0);
    if !magnitude.is_finite() || magnitude < STICK_EPSILON || !forward.is_finite() {
        return Vec2::ZERO;
    }
    let speed = match config.speed {
        SpeedSource::Game => game_speed.filter(|s| s.is_finite()).unwrap_or(0.0),
        SpeedSource::Fixed => config.run_speed_mps * magnitude,
    };
    if speed <= 0.0 {
        return Vec2::ZERO;
    }
    to_world(stick / stick.length(), forward) * speed
}

/// A stick shorter than this has no direction.
const STICK_EPSILON: f32 = 0.05;

#[cfg(test)]
mod tests {
    use super::*;

    const RATES: Rates = Rates {
        accelerate: 10.0,
        decelerate: 14.0,
    };
    const DT: f32 = 1.0 / 30.0;

    #[test]
    fn a_coast_follows_the_body_down() {
        // The end of a fast reel: the integrator starts far faster than the stick asks, and the
        // ground has held the body to a fraction of it.
        let coast = Vec2::new(0.0, 85.0);
        let target = Vec2::new(0.0, 6.0);
        let limited = coast_limited(coast, target, Some(Vec2::new(0.0, 21.0)));
        assert!((limited - Vec2::new(0.0, 21.0)).length() < 1e-4);
        // A body stopped dead leaves the stick's speed, which the step then reaches.
        let limited = coast_limited(coast, target, Some(Vec2::ZERO));
        assert!((limited.length() - 6.0).abs() < 1e-4);
    }

    #[test]
    fn the_sticks_own_velocity_is_kept() {
        let velocity = Vec2::new(3.0, 4.0);
        let target = Vec2::new(0.0, 6.0);
        assert_eq!(coast_limited(velocity, target, Some(Vec2::ZERO)), velocity);
        assert_eq!(coast_limited(velocity, target, None), velocity);
        let coast = Vec2::new(0.0, 40.0);
        assert_eq!(
            coast_limited(coast, target, Some(Vec2::new(f32::NAN, 0.0))),
            coast
        );
        // A body at least as fast as the coast keeps the coast whole.
        assert_eq!(
            coast_limited(coast, target, Some(Vec2::new(0.0, 50.0))),
            coast
        );
    }

    #[test]
    fn a_tick_never_changes_the_velocity_by_more_than_the_rate() {
        let targets = [
            Vec2::new(4.5, 0.0),
            Vec2::new(-4.5, 0.0),
            Vec2::new(0.0, 4.5),
            Vec2::ZERO,
            Vec2::new(3.0, -3.0),
        ];
        let mut velocity = Vec2::ZERO;
        for target in targets.iter().cycle().take(200) {
            let next = step(velocity, *target, DT, RATES);
            assert!((next - velocity).length() <= RATES.decelerate * DT + 1e-5);
            velocity = next;
        }
    }

    #[test]
    fn it_reaches_the_target_and_holds_it() {
        let target = Vec2::new(0.0, -4.5);
        let mut velocity = Vec2::ZERO;
        let mut ticks = 0;
        while velocity != target {
            velocity = step(velocity, target, DT, RATES);
            ticks += 1;
            assert!(ticks < 100, "never arrived");
        }
        // 4.5 m/s at 10 m/s^2 takes 0.45 s, fourteen ticks at 30 Hz.
        assert_eq!(ticks, 14);
        assert_eq!(step(velocity, target, DT, RATES), target);
    }

    #[test]
    fn it_stops_without_overshooting() {
        let mut velocity = Vec2::new(4.5, 0.0);
        let mut previous = velocity.x;
        for _ in 0..100 {
            velocity = step(velocity, Vec2::ZERO, DT, RATES);
            assert!(velocity.x >= 0.0 && velocity.x <= previous);
            assert_eq!(velocity.y, 0.0);
            previous = velocity.x;
        }
        assert_eq!(velocity, Vec2::ZERO);
    }

    #[test]
    fn a_turn_at_constant_speed_is_rate_limited() {
        let velocity = Vec2::new(4.5, 0.0);
        let next = step(velocity, Vec2::new(0.0, 4.5), DT, RATES);
        assert!(((next - velocity).length() - RATES.accelerate * DT).abs() < 1e-5);
    }

    #[test]
    fn reversing_uses_the_deceleration() {
        let velocity = Vec2::new(4.5, 0.0);
        let next = step(velocity, Vec2::new(-4.5, 0.0), DT, RATES);
        assert!(((velocity.x - next.x) - RATES.decelerate * DT).abs() < 1e-5);
    }

    #[test]
    fn bad_inputs_do_not_poison_the_velocity() {
        let velocity = Vec2::new(1.0, 2.0);
        assert_eq!(step(velocity, Vec2::NAN, DT, RATES), velocity);
        assert_eq!(step(velocity, Vec2::X, f32::NAN, RATES), velocity);
        assert_eq!(step(velocity, Vec2::X, -1.0, RATES), velocity);
        assert_eq!(step(Vec2::NAN, Vec2::X, DT, RATES), Vec2::ZERO);
    }

    #[test]
    fn the_stick_turns_with_the_frame() {
        let forward = Vec2::new(0.0, -1.0);
        assert!(to_world(Vec2::new(0.0, 1.0), forward).abs_diff_eq(forward, 1e-6));
        assert!(to_world(Vec2::new(1.0, 0.0), forward).abs_diff_eq(Vec2::new(1.0, 0.0), 1e-6));
        let east = Vec2::new(1.0, 0.0);
        assert!(to_world(Vec2::new(1.0, 0.0), east).abs_diff_eq(Vec2::new(0.0, 1.0), 1e-6));
    }

    #[test]
    fn the_game_speed_sets_the_magnitude_and_zero_stops() {
        let config = StickConfig::new();
        let forward = Vec2::new(0.0, -1.0);
        let half = Vec2::new(0.0, 0.5);
        let velocity = target(half, forward, Some(1.6), &config);
        assert!(velocity.abs_diff_eq(Vec2::new(0.0, -1.6), 1e-6));
        assert_eq!(target(half, forward, Some(0.0), &config), Vec2::ZERO);
        assert_eq!(target(half, forward, None, &config), Vec2::ZERO);
        assert_eq!(target(Vec2::ZERO, forward, Some(4.5), &config), Vec2::ZERO);
    }

    #[test]
    fn the_fixed_speed_scales_with_the_stick() {
        let config = StickConfig {
            speed: SpeedSource::Fixed,
            ..StickConfig::new()
        };
        let velocity = target(Vec2::new(0.5, 0.0), Vec2::new(0.0, -1.0), None, &config);
        assert!((velocity.length() - config.run_speed_mps * 0.5).abs() < 1e-5);
    }
}
