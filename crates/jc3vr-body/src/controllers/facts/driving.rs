//! Whether the driver's view sits at its standard place behind the wheel, as the vehicle handling
//! publishes it for the roomscale and the camera: the correction the input tick measured for the
//! driven seat ([`publish`]), which the frame half of the pose composition eases toward and adds
//! to the composed cockpit position ([`advance`]).
//!
//! The vehicle handling measures the seat and derives the correction
//! (`controllers::vehicles::driving_view`). A correction the input tick stopped publishing
//! goes stale, and the view eases back.

use std::time::Instant;

use body_rig::math::smooth::smoothstep;
use glam::Vec3;
use parking_lot::Mutex;

use crate::{controllers::roomscale::tuning::ROOMSCALE, tuning::FRESHNESS};

/// Publish the input tick's correction for the driven seat, cockpit frame, metres, or `None` out
/// of a measured driver's seat. A leaf slot, taken briefly.
pub fn publish(applied: Option<Vec3>) {
    let mut shared = SHARED.lock();
    shared.target = applied;
    shared.published_at = Some(Instant::now());
}

/// The frame half's inputs to [`advance`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameInputs {
    /// Whether the player is not entering, shuffling between, or leaving a seat.
    pub seated: bool,
    /// Seconds since the previous frame.
    pub dt: f32,
}

/// Ease the correction toward the published target and return it, cockpit frame, metres, to add
/// to the composed cockpit position. Called once per rendered VR frame by the pose composition,
/// under the roomscale frame lock; takes only this module's slot.
pub fn advance(enabled: bool, inputs: FrameInputs, now: Instant) -> Vec3 {
    let mut shared = SHARED.lock();
    let fresh = shared.published_at.is_some_and(|at| {
        now.saturating_duration_since(at) < FRESHNESS.get().facts.driving_ms.duration()
    });
    let goal = match shared.target {
        Some(applied) if enabled && inputs.seated && fresh => applied,
        _ => Vec3::ZERO,
    };
    let offset = shared
        .ease
        .advance(goal, inputs.dt, ROOMSCALE.get().driving_view.blend_s);
    shared.offset = offset;
    shared.goal = goal;
    offset
}

/// What the frame half eased toward and composed with.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EasedCorrection {
    /// The correction the view is easing toward.
    pub goal: Vec3,
    /// The correction the view was composed with on the latest frame.
    pub offset: Vec3,
}

#[inline]
pub fn status() -> EasedCorrection {
    let shared = SHARED.lock();
    EasedCorrection {
        goal: shared.goal,
        offset: shared.offset,
    }
}

/// An eased move from one correction to another, restarted from wherever it is whenever the goal
/// changes, so a new goal never jumps the view.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Ease {
    from: Vec3,
    to: Vec3,
    elapsed: f32,
}

impl Ease {
    const fn new() -> Self {
        Self {
            from: Vec3::ZERO,
            to: Vec3::ZERO,
            elapsed: 0.0,
        }
    }

    /// Step toward `goal` by `dt` seconds, over `duration` seconds, and return the value.
    fn advance(&mut self, goal: Vec3, dt: f32, duration: f32) -> Vec3 {
        if goal != self.to {
            self.from = self.value(duration);
            self.to = goal;
            self.elapsed = 0.0;
        }
        if dt.is_finite() {
            self.elapsed += dt.max(0.0);
        }
        self.value(duration)
    }

    fn value(&self, duration: f32) -> Vec3 {
        if !(duration.is_finite() && duration > 0.0) {
            return self.to;
        }
        self.from.lerp(self.to, smoothstep(self.elapsed / duration))
    }
}

struct Shared {
    target: Option<Vec3>,
    published_at: Option<Instant>,
    ease: Ease,
    goal: Vec3,
    offset: Vec3,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    target: None,
    published_at: None,
    ease: Ease::new(),
    goal: Vec3::ZERO,
    offset: Vec3::ZERO,
});

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    /// The ease starts and ends at rest and reaches the goal after its duration.
    #[test]
    fn the_ease_reaches_the_goal_smoothly() {
        let goal = Vec3::new(0.0, 0.1, -0.2);
        let mut ease = Ease::new();
        let first = ease.advance(goal, 0.0, 1.0);
        assert_eq!(first, Vec3::ZERO);
        let mid = ease.advance(goal, 0.5, 1.0);
        assert!((mid - goal * 0.5).length() < EPS);
        let early = Ease {
            from: Vec3::ZERO,
            to: goal,
            elapsed: 0.05,
        }
        .value(1.0);
        assert!(early.length() < goal.length() * 0.05, "slow out of rest");
        assert!((ease.advance(goal, 0.6, 1.0) - goal).length() < EPS);
        assert!((ease.advance(goal, 5.0, 1.0) - goal).length() < EPS);
    }

    /// A goal changed part way restarts from where the view is, so it never jumps.
    #[test]
    fn a_new_goal_starts_from_the_current_value() {
        let mut ease = Ease::new();
        let goal = Vec3::new(0.0, 0.0, -0.2);
        let partway = ease.advance(goal, 0.3, 1.0);
        let restarted = ease.advance(Vec3::ZERO, 0.0, 1.0);
        assert!((restarted - partway).length() < EPS);
        assert!(ease.advance(Vec3::ZERO, 1.0, 1.0).length() < EPS);
    }

    #[test]
    fn a_zero_duration_or_bad_step_is_immediate_and_safe() {
        let goal = Vec3::new(0.1, 0.0, 0.0);
        let mut ease = Ease::new();
        assert_eq!(ease.advance(goal, 0.0, 0.0), goal);
        let mut ease = Ease::new();
        let value = ease.advance(goal, f32::NAN, 1.0);
        assert!(value.is_finite());
    }
}
