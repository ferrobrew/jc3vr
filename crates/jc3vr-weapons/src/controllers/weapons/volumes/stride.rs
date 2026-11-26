//! The belt held steady through a stride.
//!
//! The belt's volumes ride the hip joints of Rico's drawn skeleton, and a gait clip swings his
//! pelvis about ten degrees each way every stride and bobs the hips by about ten centimetres. A
//! hand reaching for a pouch while running met a pouch that moved under it: on the 14 November take
//! three sprinting grabs at 2043 to 2046 s missed by 2 to 10 cm, and the grab standing still took.
//! While Rico moves, the hip joints the volumes are built on are low-passed over about a stride, so
//! the belt keeps the stride's average place; standing still, they follow the skeleton at once.

use body_rig::smooth::critically_damped;
use glam::Vec3;
use jc3vr_body::controllers::body::rig::frames::SkeletonLandmarks;
use jc3vr_core::config::engine::{Invalid, positive};
use serde::Deserialize;

use crate::controllers::weapons::hold::tuning::HOLD;

/// How the belt is held steady through a stride.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrideTuning {
    /// The smoothing of the hip joints while moving, seconds: about half a running stride, which
    /// takes out most of a stride's swing and bob.
    pub smoothing_s: f32,
    /// The furthest a smoothed hip joint lags the drawn one, metres: a stride's swing at the hip.
    pub max_lag_m: f32,
    /// The longest tick the filter steps, seconds: a stall resets nothing, but steps no further.
    pub max_dt_s: f32,
}

impl StrideTuning {
    pub(crate) fn validate(&self) -> Result<(), Invalid> {
        positive("volumes.stride.smoothing_s", self.smoothing_s, 5.0)?;
        positive("volumes.stride.max_lag_m", self.max_lag_m, 1.0)?;
        positive("volumes.stride.max_dt_s", self.max_dt_s, 1.0)
    }
}

/// Low-passes the hip joints of the skeleton the belt rides while Rico moves.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StrideFilter {
    state: Option<Smoothed>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Smoothed {
    hips: [Vec3; 2],
    rates: [Vec3; 2],
}

impl StrideFilter {
    pub const fn new() -> Self {
        Self { state: None }
    }

    /// The skeleton the belt rides this tick of `dt` seconds: `skeleton` with its hip joints
    /// low-passed while `moving`, and as they are otherwise. The filter's state stays in the
    /// skeleton's own frame, the cockpit's, which the root carries, so a run's forward motion is
    /// not lag.
    pub fn step(
        &mut self,
        skeleton: SkeletonLandmarks,
        moving: bool,
        dt: f32,
    ) -> SkeletonLandmarks {
        let tuning = &HOLD.get().volumes.stride;
        let dt = if dt.is_finite() {
            dt.clamp(0.0, tuning.max_dt_s)
        } else {
            0.0
        };
        if !moving || !skeleton.is_finite() {
            self.state = skeleton.is_finite().then_some(Smoothed {
                hips: skeleton.hips,
                rates: [Vec3::ZERO; 2],
            });
            return skeleton;
        }
        let state = self.state.get_or_insert(Smoothed {
            hips: skeleton.hips,
            rates: [Vec3::ZERO; 2],
        });
        for side in 0..2 {
            let (hip, rate) = damp(
                state.hips[side],
                state.rates[side],
                skeleton.hips[side],
                tuning.smoothing_s,
                dt,
            );
            state.hips[side] = hip;
            state.rates[side] = rate;
        }
        // A smoothed joint never strays further from the drawn one than a stride's swing: a turn
        // or a crouch passes through.
        for side in 0..2 {
            let offset = state.hips[side] - skeleton.hips[side];
            if offset.length() > tuning.max_lag_m {
                state.hips[side] = skeleton.hips[side] + offset.normalize() * tuning.max_lag_m;
            }
        }
        SkeletonLandmarks {
            hips: state.hips,
            ..skeleton
        }
    }
}

/// The critically damped step of each axis of `value` toward `target`, with a time constant of
/// `smoothing_s`.
fn damp(value: Vec3, rate: Vec3, target: Vec3, smoothing_s: f32, dt: f32) -> (Vec3, Vec3) {
    let axis = |i: usize| critically_damped(value[i], rate[i], target[i], smoothing_s, dt);
    let (x, y, z) = (axis(0), axis(1), axis(2));
    (Vec3::new(x.0, y.0, z.0), Vec3::new(x.1, y.1, z.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skeleton(hip_y: f32, swing: f32) -> SkeletonLandmarks {
        SkeletonLandmarks {
            head: Vec3::new(0.0, 1.7, 0.0),
            pelvis: Vec3::new(0.0, hip_y, 0.0),
            hips: [Vec3::new(-0.1, hip_y, swing), Vec3::new(0.1, hip_y, -swing)],
            shoulders: [Vec3::new(-0.2, 1.45, 0.0), Vec3::new(0.2, 1.45, 0.0)],
        }
    }

    #[test]
    fn a_strides_swing_and_bob_are_mostly_taken_out_while_moving() {
        let mut filter = StrideFilter::new();
        let dt = 1.0 / 60.0;
        let mut worst = 0.0_f32;
        for tick in 0..240 {
            // A stride of 0.7 s: the hips bob 5 cm each way and swing 6 cm.
            let phase = tick as f32 * dt * std::f32::consts::TAU / 0.7;
            let out = filter.step(
                skeleton(0.95 + 0.05 * phase.sin(), 0.06 * phase.cos()),
                true,
                dt,
            );
            if tick > 60 {
                worst = worst.max((out.hips[0] - Vec3::new(-0.1, 0.95, 0.0)).length());
            }
        }
        assert!(worst < 0.06, "the belt keeps the stride's average: {worst}");
    }

    #[test]
    fn standing_still_the_belt_follows_the_skeleton_at_once() {
        let mut filter = StrideFilter::new();
        let moved = skeleton(0.6, 0.0);
        filter.step(skeleton(0.95, 0.0), true, 1.0 / 60.0);
        assert_eq!(filter.step(moved, false, 1.0 / 60.0), moved);
    }

    #[test]
    fn a_crouch_while_moving_is_followed_within_the_lag() {
        let mut filter = StrideFilter::new();
        filter.step(skeleton(0.95, 0.0), true, 1.0 / 60.0);
        let out = filter.step(skeleton(0.5, 0.0), true, 1.0 / 60.0);
        assert!((out.hips[0].y - 0.5).abs() <= HOLD.get().volumes.stride.max_lag_m + 1e-5);
    }
}
