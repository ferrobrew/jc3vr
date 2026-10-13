//! The moving regime's walk: its stride, duty, and lift for a speed, and where a swinging foot
//! lands. Pure.
//!
//! The stride grows with the speed at a human scale, the duty factor falls from a walk's to a
//! run's, and the lift rises, so a faster walk takes longer strides at a quicker cadence and spends
//! less of each cycle on the ground, until the run has a moment with both feet in the air. A crouch
//! shortens the stride and lowers the lift by how far the hips have dropped, and keeps both feet
//! down for more of the cycle ([`CrouchGait`]).
//!
//! A swinging foot lands where the hip above it will be when it lands, plus half a stance along
//! the measured velocity, so it passes under the hip at mid-stance, plus a capture term on the
//! velocity's change, so a start or a stop puts the foot where it catches the body. The lead from
//! the hip is bounded by a share of the leg's length: the foot never lands further out than a leg
//! reaches comfortably.

use glam::Vec3;

use crate::controllers::body::procedural::config::ProceduralBodyConfig;

/// The walk's tuning at the current speed.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GaitTuning {
    /// The distance the body travels in one full cycle (a step of each foot), metres.
    pub stride_m: f32,
    /// The share of a cycle each foot spends on the ground.
    pub duty: f32,
    /// How high a step lifts the foot at its middle, metres.
    pub height_m: f32,
}

impl GaitTuning {
    /// How long one cycle takes at `speed`, seconds.
    pub fn period(&self, speed: f32) -> f32 {
        if speed > 1e-3 && speed.is_finite() {
            self.stride_m / speed
        } else {
            f32::INFINITY
        }
    }
}

/// The walk's tuning at `speed` metres per second, for a crouch `crouch` of the way to the hips'
/// lowest.
pub fn tuning(speed: f32, crouch: f32, crouch_gait: &CrouchGait) -> GaitTuning {
    let speed = if speed.is_finite() {
        speed.max(0.0)
    } else {
        0.0
    };
    let stride = (STRIDE_BASE_M + STRIDE_PER_MPS * speed).clamp(MIN_STRIDE_M, MAX_STRIDE_M);
    let t = ((speed - WALK_MPS) / (RUN_MPS - WALK_MPS)).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let upright = GaitTuning {
        stride_m: stride,
        duty: lerp(WALK_DUTY, RUN_DUTY),
        height_m: lerp(WALK_LIFT_M, RUN_LIFT_M),
    };
    crouch_gait.scale(upright, crouch)
}

/// What the landing needs beyond the gait.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LandingInput {
    /// The root's measured horizontal velocity, metres per second.
    pub velocity: Vec3,
    /// The measured velocity smoothed over a third of a second: the capture term acts only on its
    /// change.
    pub mean_velocity: Vec3,
    /// The hips' height over the ground, metres, for the capture point's time constant.
    pub hip_height_m: f32,
    /// The leg's length, metres.
    pub leg_length_m: f32,
    /// The capture term's weight.
    pub capture_gain: f32,
    /// The furthest a foot lands from its hip, as a share of the leg's length.
    pub reach_share: f32,
}

/// How far ahead of its hip's ground point at the landing a swinging foot lands: half a stance
/// along the velocity, plus the capture term on the velocity's change, bounded by the reach.
pub fn landing_lead(gait: &GaitTuning, input: &LandingInput) -> Vec3 {
    let speed = flat(input.velocity).length();
    let period = gait.period(speed);
    let half_stance = if period.is_finite() {
        flat(input.velocity) * (gait.duty * period * 0.5)
    } else {
        Vec3::ZERO
    };
    let capture = flat(input.velocity - input.mean_velocity)
        * (input.capture_gain * capture_time(input.hip_height_m));
    let reach = (input.leg_length_m * input.reach_share).max(0.0);
    let lead = (half_stance + capture).clamp_length_max(reach);
    if lead.is_finite() { lead } else { Vec3::ZERO }
}

/// The capture point's time constant for hips `height` metres up: `√(h/g)`, seconds.
pub fn capture_time(height: f32) -> f32 {
    let height = if height.is_finite() {
        height.max(0.0)
    } else {
        0.0
    };
    (height / GRAVITY).sqrt()
}

/// How a crouch changes the walk: at the hips' lowest, the stride and the lift are these shares of
/// the upright walk's, and each foot spends at least `duty` of the cycle on the ground.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CrouchGait {
    pub stride_share: f32,
    pub lift_share: f32,
    pub duty: f32,
}

impl CrouchGait {
    /// The crouch gait the settings ask for: no change with `crouch_gait` off.
    pub fn of(config: &ProceduralBodyConfig) -> Self {
        if config.crouch_gait {
            Self {
                stride_share: config.crouch_stride_share,
                lift_share: config.crouch_lift_share,
                duty: config.crouch_duty,
            }
        } else {
            Self::UPRIGHT
        }
    }

    /// A crouch that changes nothing.
    pub const UPRIGHT: Self = Self {
        stride_share: 1.0,
        lift_share: 1.0,
        duty: 0.0,
    };

    /// `upright` scaled for a crouch `crouch` of the way to the hips' lowest, eased in so that a
    /// shallow dip of the head barely changes the walk.
    pub fn scale(&self, upright: GaitTuning, crouch: f32) -> GaitTuning {
        let c = if crouch.is_finite() {
            crouch.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let c = c * c * (3.0 - 2.0 * c);
        let lerp = |a: f32, b: f32| a + (b - a) * c;
        GaitTuning {
            stride_m: upright.stride_m * lerp(1.0, self.stride_share.clamp(0.1, 1.0)),
            duty: lerp(upright.duty, upright.duty.max(self.duty.clamp(0.0, 0.9))),
            height_m: upright.height_m * lerp(1.0, self.lift_share.clamp(0.0, 1.0)),
        }
    }

    /// The share of a standing step's lift a crouch `crouch` deep leaves.
    pub fn step_lift(&self, crouch: f32) -> f32 {
        let unit = GaitTuning {
            stride_m: 1.0,
            duty: 0.5,
            height_m: 1.0,
        };
        self.scale(unit, crouch).height_m
    }
}

/// The stride at standstill, and what each metre per second of speed adds to it, metres.
const STRIDE_BASE_M: f32 = 0.9;
const STRIDE_PER_MPS: f32 = 0.35;
/// The shortest and the longest stride, metres per cycle.
const MIN_STRIDE_M: f32 = 0.7;
const MAX_STRIDE_M: f32 = 2.0;
/// The walking and the running speeds the duty and the lift are set at, metres per second.
const WALK_MPS: f32 = 1.4;
const RUN_MPS: f32 = 5.0;
/// The duty factor at the walking and the running speeds.
const WALK_DUTY: f32 = 0.62;
const RUN_DUTY: f32 = 0.32;
/// The lift at the walking and the running speeds, metres.
const WALK_LIFT_M: f32 = 0.08;
const RUN_LIFT_M: f32 = 0.14;
/// Gravity, metres per second squared.
const GRAVITY: f32 = 9.81;

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn landing(velocity: Vec3, mean_velocity: Vec3) -> LandingInput {
        LandingInput {
            velocity,
            mean_velocity,
            hip_height_m: 0.95,
            leg_length_m: 0.9,
            capture_gain: 0.5,
            reach_share: 0.42,
        }
    }

    #[test]
    fn faster_takes_longer_strides_and_less_time_on_the_ground() {
        let walk = tuning(1.4, 0.0, &CrouchGait::UPRIGHT);
        let run = tuning(5.0, 0.0, &CrouchGait::UPRIGHT);
        assert!((walk.stride_m - 1.39).abs() < 1e-4);
        assert_eq!(run.stride_m, 2.0);
        assert!((walk.duty - 0.62).abs() < 1e-6);
        assert!((run.duty - 0.32).abs() < 1e-6);
        assert!(run.height_m > walk.height_m);
        assert!((tuning(0.1, 0.0, &CrouchGait::UPRIGHT).stride_m - 0.935).abs() < 1e-5);
    }

    #[test]
    fn a_crouch_shortens_and_lowers_the_walk() {
        let crouch = CrouchGait {
            stride_share: 0.5,
            lift_share: 0.5,
            duty: 0.7,
        };
        let low = tuning(1.4, 1.0, &crouch);
        let upright = tuning(1.4, 0.0, &crouch);
        assert!((low.stride_m - upright.stride_m * 0.5).abs() < 1e-5);
        assert!((low.duty - 0.7).abs() < 1e-6);
        assert!((crouch.step_lift(1.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_lead_is_half_a_stance_and_bounded_by_the_reach() {
        let walk = tuning(0.8, 0.0, &CrouchGait::UPRIGHT);
        let v = Vec3::new(0.0, 0.0, -0.8);
        let lead = landing_lead(&walk, &landing(v, v));
        assert!(
            (lead.length() - walk.duty * walk.stride_m * 0.5).abs() < 1e-4,
            "{lead}"
        );
        assert!(lead.z < 0.0);
        // Accelerating hard: the capture term pushes the foot further, up to the reach.
        let sprint = Vec3::new(0.0, 0.0, -8.0);
        let lunge = landing_lead(
            &tuning(8.0, 0.0, &CrouchGait::UPRIGHT),
            &landing(sprint, Vec3::ZERO),
        );
        assert!((lunge.length() - 0.42 * 0.9).abs() < 1e-5);
        assert_eq!(
            landing_lead(&walk, &landing(Vec3::ZERO, Vec3::ZERO)),
            Vec3::ZERO
        );
    }
}
