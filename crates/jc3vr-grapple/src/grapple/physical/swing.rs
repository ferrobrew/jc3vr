//! The hang swing's pump: the grapple hand's motion relative to the head pushes a hanging
//! character the other way, as a body hanging from a rope moves against the hand that holds it.
//! Pure; the hang task's detour in the hands' anchor applies it with the hold that the hand's
//! displacement drives, through [`HANG_PUMP`], which the grapple's registration fills there
//! ([`crate::register`]).
//!
//! The hold pushes for as long as the hand stays off where it took the line, like a stick held
//! over. The pump pushes only while the hand moves, so pulling the hand back as the body swings
//! forward, and reaching out as it swings back, feeds the swing the way a player pumps a rope. Both
//! are measured against the game's own continuous stick swing at full deflection, and their sum is
//! capped per hang tick so that a flailing hand cannot fling the body.

use body_rig::math::smooth::follow_factor;
use glam::Vec2;
use jc3vr_body::controllers::hands::anchor::HangPump;

use crate::grapple::{physical::config::SwingTuning, tuning::GRAPPLE};

/// The pump as the hands' anchor calls it, with the fixed tuning.
pub(crate) const HANG_PUMP: HangPump = HangPump {
    smooth_velocity: anchor_smooth_velocity,
    impulse: anchor_pump_impulse,
    combine: anchor_combine,
};

/// The pump for a grapple hand moving at `velocity` (horizontal, body frame, metres per second)
/// relative to the head: against the motion, from zero at the deadzone to `full_impulse` times
/// [`SwingTuning::scale`] at the full speed, and zero inside the deadzone.
pub fn pump_impulse(velocity: Vec2, full_impulse: f32, config: &SwingTuning) -> Vec2 {
    let speed = velocity.length();
    if !speed.is_finite() || speed <= config.deadzone_mps {
        return Vec2::ZERO;
    }
    let span = (config.full_mps - config.deadzone_mps).max(1e-3);
    let strength = ((speed - config.deadzone_mps) / span).clamp(0.0, 1.0);
    -velocity / speed * strength * full_impulse * config.scale
}

/// The hold and the pump together, capped at `full_impulse` times [`SwingTuning::max_scale`].
pub fn combine(hold: Vec2, pump: Vec2, full_impulse: f32, config: &SwingTuning) -> Vec2 {
    let total = hold + pump;
    if !total.is_finite() {
        return Vec2::ZERO;
    }
    total.clamp_length_max((full_impulse * config.max_scale).abs())
}

/// Smooth the hand's velocity toward `sample` over `dt` seconds.
pub fn smooth_velocity(previous: Vec2, sample: Vec2, dt: f32, config: &SwingTuning) -> Vec2 {
    if !sample.is_finite() {
        return previous;
    }
    previous.lerp(sample, follow_factor(dt, config.smoothing_s))
}

fn anchor_smooth_velocity(previous: Vec2, sample: Vec2, dt: f32) -> Vec2 {
    smooth_velocity(previous, sample, dt, &GRAPPLE.get().physical.swing)
}

fn anchor_pump_impulse(velocity: Vec2, full_impulse: f32) -> Vec2 {
    pump_impulse(velocity, full_impulse, &GRAPPLE.get().physical.swing)
}

fn anchor_combine(hold: Vec2, pumped: Vec2, full_impulse: f32) -> Vec2 {
    combine(hold, pumped, full_impulse, &GRAPPLE.get().physical.swing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pump_pushes_against_the_motion_and_saturates() {
        let config = GRAPPLE.get().physical.swing;
        assert_eq!(pump_impulse(Vec2::new(0.1, 0.0), 10.0, &config), Vec2::ZERO);
        let half_speed = (config.deadzone_mps + config.full_mps) / 2.0;
        let half = pump_impulse(Vec2::new(0.0, half_speed), 10.0, &config);
        assert!((half.y + 5.0 * config.scale).abs() < 1e-3, "{half}");
        let full = pump_impulse(Vec2::new(-9.0, 0.0), 10.0, &config);
        assert!((full.x - 10.0 * config.scale).abs() < 1e-3, "{full}");
        assert_eq!(
            pump_impulse(Vec2::new(f32::NAN, 0.0), 10.0, &config),
            Vec2::ZERO
        );
    }

    #[test]
    fn the_sum_is_capped() {
        let config = GRAPPLE.get().physical.swing;
        let total = combine(Vec2::new(10.0, 0.0), Vec2::new(30.0, 0.0), 10.0, &config);
        assert!((total.length() - 10.0 * config.max_scale).abs() < 1e-3);
        let small = combine(Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), 10.0, &config);
        assert_eq!(small, Vec2::new(1.0, 1.0));
        assert_eq!(
            combine(Vec2::new(f32::NAN, 0.0), Vec2::ZERO, 10.0, &config),
            Vec2::ZERO
        );
    }

    #[test]
    fn the_velocity_settles_on_the_samples() {
        let config = GRAPPLE.get().physical.swing;
        let mut v = Vec2::ZERO;
        for _ in 0..30 {
            v = smooth_velocity(v, Vec2::new(1.0, 0.0), 1.0 / 30.0, &config);
        }
        assert!((v.x - 1.0).abs() < 1e-3);
        assert_eq!(
            smooth_velocity(v, Vec2::new(f32::NAN, 0.0), 0.1, &config),
            v
        );
    }
}
