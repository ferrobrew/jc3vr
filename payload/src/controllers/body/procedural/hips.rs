//! The clip's hips against Rico's facing, for squaring Rico's stance: the idle clip stands bladed,
//! its hips turned a quarter of a right angle from the root, so the yaw follow steers the root by
//! the hips' heading and the hips, rather than the root, face where the player's legs do
//! (`docs/mod/body/procedural-body.md`).
//!
//! The gate measures the heading of the line across the clip's upper legs in Rico's model space,
//! before the IK solves, while Rico is on foot, and follows it slowly enough to take out a walk's
//! swing of the hips from stride to stride.

use std::time::{Duration, Instant};

use body_rig::angle::{line_yaw, wrap};
use parking_lot::Mutex;

use crate::controllers::body::procedural::{rig::RigBone, solver::pose::RigPose};

/// The clip's hips' heading against Rico's facing, radians, positive turned to his left, while the
/// gate measured it recently on foot.
pub fn hip_yaw() -> Option<f32> {
    let now = Instant::now();
    PUBLISHED
        .lock()
        .filter(|(_, at)| now.saturating_duration_since(*at) <= FRESH)
        .map(|(yaw, _)| yaw)
}

/// The followed heading of the clip's hips.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct HipFollower {
    yaw: Option<f32>,
}

impl HipFollower {
    pub(super) const fn new() -> Self {
        Self { yaw: None }
    }

    /// Fold in the hips of the animated pose `animated` over `dt` seconds, and publish the result.
    pub(super) fn observe(&mut self, animated: &RigPose, dt: f32, now: Instant) {
        let measured = line_yaw(
            animated.position(RigBone::LeftUpLeg),
            animated.position(RigBone::RightUpLeg),
        );
        let yaw = self.step(measured, dt);
        *PUBLISHED.lock() = yaw.map(|yaw| (yaw, now));
    }

    /// Stop following, off the ground or with the follow switched off.
    pub(super) fn reset(&mut self) {
        self.yaw = None;
        *PUBLISHED.lock() = None;
    }

    /// Follow `measured` over `dt` seconds: the first measurement is taken whole, and later ones
    /// are followed the short way round with a time constant of [`SMOOTHING_S`].
    fn step(&mut self, measured: Option<f32>, dt: f32) -> Option<f32> {
        let measured = measured.filter(|yaw| yaw.is_finite())?;
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let yaw = match self.yaw {
            Some(yaw) => wrap(yaw + wrap(measured - yaw) * (dt / (SMOOTHING_S + dt))),
            None => wrap(measured),
        };
        self.yaw = Some(yaw);
        self.yaw
    }
}

/// The hips' follower's time constant, seconds: long against a stride, so the walk's swing of the
/// hips averages out, and short against standing still.
const SMOOTHING_S: f32 = 0.8;

/// How long a published heading counts, a few ticks.
const FRESH: Duration = Duration::from_millis(500);

static PUBLISHED: Mutex<Option<(f32, Instant)>> = Mutex::new(None);

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn the_first_measurement_is_taken_whole() {
        let mut follower = HipFollower::new();
        assert!((follower.step(Some(-0.4), DT).unwrap() + 0.4).abs() < 1e-6);
        assert_eq!(follower.step(None, DT), None);
        assert!((follower.yaw.unwrap() + 0.4).abs() < 1e-6);
    }

    #[test]
    fn a_stride_swing_averages_out_and_a_new_stance_is_reached() {
        let mut follower = HipFollower::new();
        follower.step(Some(0.0), DT);
        // A walk swinging the hips ten degrees either way, about once a second.
        let swing = 10.0f32.to_radians();
        let mut furthest = 0.0f32;
        for i in 0..600 {
            let t = i as f32 * DT;
            let yaw = follower
                .step(Some(swing * (t * std::f32::consts::TAU).sin()), DT)
                .unwrap();
            if t > 2.0 {
                furthest = furthest.max(yaw.abs());
            }
        }
        assert!(furthest < swing * 0.25, "{}", furthest.to_degrees());
        // Standing in the bladed idle, the follower reaches it within a few seconds.
        let idle = -24.0f32.to_radians();
        let mut yaw = 0.0;
        for _ in 0..(4.0 / DT) as usize {
            yaw = follower.step(Some(idle), DT).unwrap();
        }
        assert!((yaw - idle).abs() < 1.0f32.to_radians());
    }

    #[test]
    fn the_follower_goes_the_short_way_round() {
        let mut follower = HipFollower::new();
        follower.step(Some(3.1), DT);
        let yaw = follower.step(Some(-3.1), 1.0).unwrap();
        assert!(yaw.abs() > 3.0, "{yaw}");
    }
}
