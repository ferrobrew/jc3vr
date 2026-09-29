//! The chase loop's arithmetic: the velocity that walks the body toward the physical head, its
//! blend with the locomotion task's own velocity, and the warp test. Pure.
//!
//! Vectors here are world-space horizontal velocities and offsets, as `(x, z)`.

use glam::Vec2;

use crate::controllers::config::ChaseConfig;

/// The chase's engagement, kept across ticks for the deadzone's hysteresis.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Chaser {
    engaged: bool,
}

impl Chaser {
    pub const fn new() -> Self {
        Self { engaged: false }
    }

    /// Whether the body is chasing the head.
    pub fn engaged(&self) -> bool {
        self.engaged
    }

    /// Stop chasing, after a teleport or whenever the chase stands down.
    pub fn reset(&mut self) {
        self.engaged = false;
    }

    /// The chase velocity for the head's horizontal `offset` from the body (metres, world space),
    /// with the move stick at `stick` (`0..=1`). The chase starts once the offset leaves the
    /// deadzone and runs until it is back within half of it; while it runs, the speed closes the
    /// offset over the time constant, up to the speed limit, and scales down as the stick is pushed
    /// so that deliberate stick locomotion takes precedence.
    pub fn velocity(&mut self, offset: Vec2, stick: f32, config: &ChaseConfig) -> Vec2 {
        let distance = offset.length();
        if !distance.is_finite() {
            self.engaged = false;
            return Vec2::ZERO;
        }
        let deadzone = config.deadzone_m.max(0.0);
        self.engaged = if self.engaged {
            distance > deadzone * 0.5
        } else {
            distance > deadzone
        };
        if !self.engaged || distance <= f32::EPSILON {
            return Vec2::ZERO;
        }
        let tau = config.time_constant_s.max(1e-3);
        let speed = (distance / tau).min(config.max_speed_mps.max(0.0));
        let stick = if stick.is_finite() {
            stick.clamp(0.0, 1.0)
        } else {
            0.0
        };
        offset / distance * speed * (1.0 - stick)
    }
}

/// The locomotion task's horizontal velocity with the chase added. The sum may not exceed the
/// faster of the task's own speed and the chase's limit, so that the chase never pushes a running
/// body past its animation's speed.
pub fn combine(task: Vec2, chase: Vec2, max_chase_speed: f32) -> Vec2 {
    let sum = task + chase;
    let limit = task.length().max(max_chase_speed.max(0.0));
    let speed = sum.length();
    if speed > limit && speed > 0.0 {
        sum * (limit / speed)
    } else {
        sum
    }
}

/// The requested displacement split before the proxy's single collision solve.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Movement {
    pub task: Vec2,
    pub chase: Vec2,
}

impl Movement {
    /// Apply the combined speed cap equally to both contributors, then integrate for this tick.
    pub fn new(task: Vec2, chase: Vec2, max_chase_speed: f32, dt: f32) -> Self {
        if !task.is_finite() || !chase.is_finite() || !dt.is_finite() || dt <= 0.0 {
            return Self::default();
        }
        let speed = (task + chase).length();
        let capped = combine(task, chase, max_chase_speed);
        let scale = if speed > 0.0 {
            capped.length() / speed
        } else {
            1.0
        };
        Self {
            task: task * (scale * dt),
            chase: chase * (scale * dt),
        }
    }

    /// Attribute free movement exactly, including opposing requests. The engine exposes only one
    /// solved proxy displacement, so collisions do not identify each contributor. For a mismatch,
    /// remove only the chase's forward projection along achieved movement, bounded by the achieved
    /// distance and the requested chase distance. This deliberately leaves ambiguous motion visible;
    /// it is not a reconstruction of the collision solver or a task-only counterfactual.
    pub fn achieved_chase(self, actual: Vec2) -> Vec2 {
        if !actual.is_finite() || !self.task.is_finite() || !self.chase.is_finite() {
            return Vec2::ZERO;
        }
        let expected = self.task + self.chase;
        if actual.distance(expected) <= 1e-4 + expected.length() * 1e-3 {
            return self.chase;
        }
        let distance = actual.length();
        if distance <= f32::EPSILON {
            return Vec2::ZERO;
        }
        let direction = actual / distance;
        direction
            * self
                .chase
                .dot(direction)
                .max(0.0)
                .min(distance)
                .min(self.chase.length())
    }
}

/// Whether a single-tick body displacement of `distance` metres, at `speed` metres per second over
/// `dt` seconds, is a teleport rather than movement: further than `min_m`, and further than three
/// times what the speed covers, so that fast vehicles and dives never trip it.
pub fn is_warp(distance: f32, speed: f32, dt: f32, min_m: f32) -> bool {
    let travelled = if speed.is_finite() && dt.is_finite() {
        3.0 * speed.abs() * dt.max(0.0)
    } else {
        0.0
    };
    distance.is_finite() && distance > min_m.max(travelled)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: ChaseConfig = ChaseConfig {
        enabled: true,
        deadzone_m: 0.1,
        max_speed_mps: 3.0,
        time_constant_s: 0.2,
        warp_min_m: 8.0,
    };

    #[test]
    fn nothing_moves_inside_the_deadzone() {
        let mut chaser = Chaser::default();
        assert_eq!(
            chaser.velocity(Vec2::new(0.05, 0.0), 0.0, &CONFIG),
            Vec2::ZERO
        );
        assert!(!chaser.engaged());
    }

    #[test]
    fn the_chase_closes_the_offset_over_the_time_constant() {
        let mut chaser = Chaser::default();
        let v = chaser.velocity(Vec2::new(0.0, 0.3), 0.0, &CONFIG);
        assert!((v - Vec2::new(0.0, 1.5)).length() < 1e-5);
    }

    #[test]
    fn the_chase_is_limited_and_yields_to_the_stick() {
        let mut chaser = Chaser::default();
        let far = chaser.velocity(Vec2::new(2.0, 0.0), 0.0, &CONFIG);
        assert!((far.length() - 3.0).abs() < 1e-5);
        let half_stick = chaser.velocity(Vec2::new(2.0, 0.0), 0.5, &CONFIG);
        assert!((half_stick.length() - 1.5).abs() < 1e-5);
        assert_eq!(
            chaser.velocity(Vec2::new(2.0, 0.0), 1.0, &CONFIG),
            Vec2::ZERO
        );
    }

    #[test]
    fn a_started_chase_runs_until_half_the_deadzone() {
        let mut chaser = Chaser::default();
        chaser.velocity(Vec2::new(0.2, 0.0), 0.0, &CONFIG);
        assert!(chaser.engaged());
        assert_ne!(
            chaser.velocity(Vec2::new(0.07, 0.0), 0.0, &CONFIG),
            Vec2::ZERO
        );
        assert_eq!(
            chaser.velocity(Vec2::new(0.04, 0.0), 0.0, &CONFIG),
            Vec2::ZERO
        );
        assert!(!chaser.engaged());
    }

    #[test]
    fn combining_never_exceeds_the_faster_speed() {
        let running = Vec2::new(0.0, 5.0);
        let sum = combine(running, Vec2::new(3.0, 0.0), 3.0);
        assert!((sum.length() - 5.0).abs() < 1e-4);
        let idle = combine(Vec2::ZERO, Vec2::new(2.0, 0.0), 3.0);
        assert_eq!(idle, Vec2::new(2.0, 0.0));
    }

    #[test]
    fn free_motion_preserves_each_capped_task_vector() {
        for (task, chase) in [
            (Vec2::X, Vec2::Y),
            (Vec2::X * 3.0, -Vec2::X),
            (Vec2::X, -Vec2::X),
            (Vec2::X * 5.0, Vec2::Y * 3.0),
            (Vec2::X, Vec2::X * 3.0),
        ] {
            let movement = Movement::new(task, chase, 3.0, 0.02);
            let actual = combine(task, chase, 3.0) * 0.02;
            assert!((actual - movement.achieved_chase(actual)).distance(movement.task) < 1e-6);
        }
    }

    #[test]
    fn blocked_and_sliding_motion_have_bounded_compensation() {
        let movement = Movement::new(Vec2::X, Vec2::Y, 3.0, 1.0);
        assert_eq!(movement.achieved_chase(Vec2::ZERO), Vec2::ZERO);
        assert_eq!(movement.achieved_chase(Vec2::X), Vec2::ZERO);
        assert_eq!(movement.achieved_chase(Vec2::Y * 0.5), Vec2::Y * 0.5);
        assert_eq!(movement.achieved_chase(-Vec2::Y), Vec2::ZERO);
        for actual in [Vec2::new(0.1, 0.2), Vec2::Y * 10.0, Vec2::new(-0.2, 0.3)] {
            let correction = movement.achieved_chase(actual);
            assert!(correction.length() <= actual.length() + 1e-6);
            assert!(correction.length() <= movement.chase.length() + 1e-6);
            assert!(correction.dot(actual) >= 0.0);
        }
    }

    #[test]
    fn a_warp_is_far_and_faster_than_the_body_moves() {
        assert!(!is_warp(0.5, 5.0, 1.0 / 30.0, 8.0));
        assert!(is_warp(20.0, 5.0, 1.0 / 30.0, 8.0));
        // A diving wingsuit covers ten metres a tick at 300 m/s.
        assert!(!is_warp(10.0, 300.0, 1.0 / 30.0, 8.0));
        assert!(!is_warp(f32::NAN, 0.0, 0.0, 8.0));
    }
}
