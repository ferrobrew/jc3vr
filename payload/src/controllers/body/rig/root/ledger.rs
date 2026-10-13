//! The chase's bookkeeping: what each simulation step asked the root to move, split between the
//! chase and everything else, and how much of the completed movement was the chase's. Pure.
//!
//! The root hook submits a [`Movement`] each tick the player owns the root; the pose-finalization
//! entry completes it against the root's displacement between the two world transforms the engine
//! interpolates ([`Requests::complete`]). The running total of credited chase movement at the last
//! two completed steps ([`CompletedMovement`]) is what the cockpit shift folds in
//! ([`crate::controllers::body::rig::root::shift`]).
//!
//! Vectors here are world-space horizontal displacements, as `(x, z)`.

use glam::{Vec2, Vec3};

/// The requested displacement split before the proxy's single collision solve.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Movement {
    pub task: Vec2,
    pub chase: Vec2,
}

impl Movement {
    /// Integrate the task's and the chase's velocities for this tick. Nothing scales the sum but a
    /// sanity cap ([`MAX_COMBINED_SPEED_MPS`]), applied to both alike so that the split survives.
    pub fn new(task: Vec2, chase: Vec2, dt: f32) -> Self {
        if !task.is_finite() || !chase.is_finite() || !dt.is_finite() || dt <= 0.0 {
            return Self::default();
        }
        let speed = (task + chase).length();
        let scale = if speed > MAX_COMBINED_SPEED_MPS {
            MAX_COMBINED_SPEED_MPS / speed
        } else {
            1.0
        };
        Self {
            task: task * (scale * dt),
            chase: chase * (scale * dt),
        }
    }

    /// The combined velocity this movement asks for over `dt`, for the proxy.
    pub fn velocity(self, dt: f32) -> Vec2 {
        if dt.is_finite() && dt > 0.0 {
            (self.task + self.chase) / dt
        } else {
            Vec2::ZERO
        }
    }

    /// Attribute free movement exactly, including opposing requests. The engine exposes only one
    /// solved proxy displacement, so collisions do not identify each contributor. For a mismatch,
    /// credit only the chase's forward projection along achieved movement, bounded by the achieved
    /// distance and the requested chase distance. This deliberately leaves ambiguous motion visible;
    /// it is not a reconstruction of the collision solver or a task-only counterfactual.
    ///
    /// `actual` is the difference of two absolute world positions, so it carries their `f32`
    /// rounding, which grows with the distance from the world origin. `position_scale` is the
    /// larger magnitude of the two positions (any coordinate bound will do), and the match
    /// tolerance widens with it so that free movement far from the origin still matches.
    pub fn achieved_chase(self, actual: Vec2, position_scale: f32) -> (Vec2, Attribution) {
        if !actual.is_finite() || !self.task.is_finite() || !self.chase.is_finite() {
            return (Vec2::ZERO, Attribution::Projected);
        }
        let expected = self.task + self.chase;
        if actual.distance(expected) <= exact_tolerance(expected.length(), position_scale) {
            return (self.chase, Attribution::Exact);
        }
        let distance = actual.length();
        if distance <= f32::EPSILON {
            return (Vec2::ZERO, Attribution::Projected);
        }
        let direction = actual / distance;
        let credited = direction
            * self
                .chase
                .dot(direction)
                .max(0.0)
                .min(distance)
                .min(self.chase.length());
        (credited, Attribution::Projected)
    }
}

/// How a completed step's chase was credited.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Attribution {
    /// The root moved exactly as asked: all of the chase is credited.
    Exact,
    /// Something stopped or deflected the root: only the chase's projection on what it achieved.
    Projected,
}

/// Cumulative world-space chase movement credited at the last two completed simulation steps.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct CompletedMovement {
    pub previous: Vec2,
    pub current: Vec2,
}

/// How many completed steps were credited each way since the payload loaded.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct LedgerCounts {
    pub exact: u64,
    pub projected: u64,
}

/// The pending request and the running total.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Requests {
    pending: Option<Movement>,
    completed: CompletedMovement,
    counts: LedgerCounts,
    /// The requested and the credited chase distances of the last two completed steps, newest
    /// last.
    recent: [(f32, f32); 2],
}

impl Requests {
    pub const fn new() -> Self {
        Self {
            pending: None,
            completed: CompletedMovement {
                previous: Vec2::ZERO,
                current: Vec2::ZERO,
            },
            counts: LedgerCounts {
                exact: 0,
                projected: 0,
            },
            recent: [(0.0, 0.0); 2],
        }
    }

    /// Whether something stopped the chase: over the last two completed steps, less than half of
    /// the chase they asked for was credited. A wall the root slides along at a shallow angle
    /// leaves most of the chase credited, so it is no obstruction.
    pub fn obstructed(&self) -> bool {
        let requested: f32 = self.recent.iter().map(|(r, _)| r).sum();
        let credited: f32 = self.recent.iter().map(|(_, c)| c).sum();
        requested > MIN_OBSTRUCTED_REQUEST_M && credited < requested * OBSTRUCTED_SHARE
    }

    /// The running total at the last two completed steps.
    pub fn completed(&self) -> CompletedMovement {
        self.completed
    }

    pub fn counts(&self) -> LedgerCounts {
        self.counts
    }

    /// Whether a request waits for its step to complete.
    #[cfg(test)]
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Ask for this tick's movement; the next completion credits it.
    pub fn submit(&mut self, movement: Movement) {
        self.pending = Some(movement);
    }

    /// Complete a step that moved the root from `previous` to `current`, world positions.
    pub fn complete(&mut self, previous: Vec3, current: Vec3) {
        self.completed.previous = self.completed.current;
        let mut step = (0.0, 0.0);
        if let Some(request) = self.pending.take() {
            let delta = current - previous;
            let position_scale = previous.abs().max(current.abs()).max_element();
            let (credited, attribution) =
                request.achieved_chase(Vec2::new(delta.x, delta.z), position_scale);
            self.completed.current += credited;
            match attribution {
                Attribution::Exact => self.counts.exact += 1,
                Attribution::Projected => self.counts.projected += 1,
            }
            step = (request.chase.length(), credited.length());
        }
        self.recent = [self.recent[1], step];
    }

    /// Drop the pending request and settle the pair, so nothing is credited until the player owns
    /// the root again.
    pub fn clear(&mut self) {
        self.pending = None;
        self.recent = [(0.0, 0.0); 2];
        self.completed.previous = self.completed.current;
    }
}

/// The fastest the stick, the hands, and the chase together may move the root, metres per second:
/// a sanity bound, well above anything a player asks for.
pub const MAX_COMBINED_SPEED_MPS: f32 = 12.0;

/// The least chase two steps must ask for, metres, before their shortfall counts as an
/// obstruction.
const MIN_OBSTRUCTED_REQUEST_M: f32 = 1e-3;

/// The share of the requested chase below which the credited chase means an obstruction.
const OBSTRUCTED_SHARE: f32 = 0.5;

/// The absolute part of the exact-match tolerance, metres.
const EXACT_ABSOLUTE_M: f32 = 1e-4;
/// The part of the exact-match tolerance relative to the requested displacement.
const EXACT_RELATIVE: f32 = 1e-3;
/// The exact-match tolerance in units in the last place of the positions' magnitude. Each of the
/// two positions rounds by up to half a unit per axis, and the engine's own integration of the
/// displacement rounds once more, so a free step can miss by about two units along its length.
const EXACT_POSITION_ULPS: f32 = 4.0;

/// How far the achieved displacement may be from the requested one and still count as free
/// movement, for a request of `expected` metres between positions of magnitude `position_scale`.
fn exact_tolerance(expected: f32, position_scale: f32) -> f32 {
    let position_scale = if position_scale.is_finite() {
        position_scale.abs()
    } else {
        0.0
    };
    EXACT_ABSOLUTE_M
        + expected * EXACT_RELATIVE
        + position_scale * f32::EPSILON * EXACT_POSITION_ULPS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chase(movement: Movement, actual: Vec2, scale: f32) -> Vec2 {
        movement.achieved_chase(actual, scale).0
    }

    #[test]
    fn nothing_scales_the_sum_below_the_sanity_cap() {
        let movement = Movement::new(Vec2::new(0.0, 5.0), Vec2::new(3.0, 0.0), 1.0);
        assert_eq!(movement.task, Vec2::new(0.0, 5.0));
        assert_eq!(movement.chase, Vec2::new(3.0, 0.0));
        let fast = Movement::new(Vec2::new(0.0, 20.0), Vec2::new(0.0, 4.0), 1.0);
        assert!(((fast.task + fast.chase).length() - MAX_COMBINED_SPEED_MPS).abs() < 1e-4);
        assert!((fast.task.y / fast.chase.y - 5.0).abs() < 1e-4);
        assert_eq!(Movement::new(Vec2::X, Vec2::X, 0.0), Movement::default());
    }

    #[test]
    fn free_motion_preserves_each_task_vector() {
        for (task, chase_v) in [
            (Vec2::X, Vec2::Y),
            (Vec2::X * 3.0, -Vec2::X),
            (Vec2::X, -Vec2::X),
            (Vec2::X * 5.0, Vec2::Y * 3.0),
            (Vec2::X, Vec2::X * 3.0),
        ] {
            let movement = Movement::new(task, chase_v, 0.02);
            let actual = (task + chase_v) * 0.02;
            let (credited, attribution) = movement.achieved_chase(actual, 0.0);
            assert_eq!(attribution, Attribution::Exact);
            assert!((actual - credited).distance(movement.task) < 1e-6);
        }
    }

    #[test]
    fn blocked_and_sliding_motion_have_bounded_compensation() {
        let movement = Movement::new(Vec2::X, Vec2::Y, 1.0);
        assert_eq!(chase(movement, Vec2::ZERO, 0.0), Vec2::ZERO);
        assert_eq!(chase(movement, Vec2::X, 0.0), Vec2::ZERO);
        assert_eq!(chase(movement, Vec2::Y * 0.5, 0.0), Vec2::Y * 0.5);
        assert_eq!(chase(movement, -Vec2::Y, 0.0), Vec2::ZERO);
        for actual in [Vec2::new(0.1, 0.2), Vec2::Y * 10.0, Vec2::new(-0.2, 0.3)] {
            let (correction, attribution) = movement.achieved_chase(actual, 0.0);
            assert_eq!(attribution, Attribution::Projected);
            assert!(correction.length() <= actual.length() + 1e-6);
            assert!(correction.length() <= movement.chase.length() + 1e-6);
            assert!(correction.dot(actual) >= 0.0);
        }
    }

    #[test]
    fn free_motion_far_from_the_origin_is_attributed_exactly() {
        let dt = 1.0 / 30.0;
        for origin in [
            Vec3::new(2_000.0, 300.0, -2_000.0),
            Vec3::new(-8_000.0, 50.0, 8_000.0),
            Vec3::new(16_000.0, 0.0, 16_000.0),
        ] {
            for (task, chase_v) in [
                (Vec2::new(0.0, 1.5), Vec2::new(0.7, 0.0)),
                (Vec2::new(0.0, 3.0), Vec2::new(0.0, -0.4)),
                (Vec2::new(1.0, 1.0), Vec2::new(-0.3, 0.2)),
            ] {
                let movement = Movement::new(task, chase_v, dt);
                let wanted = movement.task + movement.chase;
                // The engine integrates the displacement onto the absolute position in `f32`.
                let current = origin + Vec3::new(wanted.x, 0.0, wanted.y);
                let solved = current - origin;
                let scale = origin.abs().max(current.abs()).max_element();
                let achieved = movement.achieved_chase(Vec2::new(solved.x, solved.z), scale);
                assert_eq!(
                    achieved,
                    (movement.chase, Attribution::Exact),
                    "at {origin} for {task} + {chase_v}"
                );
            }
        }
    }

    #[test]
    fn a_real_shortfall_far_from_the_origin_is_not_exact() {
        let movement = Movement::new(Vec2::new(0.0, 1.5), Vec2::new(0.9, 0.0), 1.0 / 30.0);
        let wanted = movement.task + movement.chase;
        // Wall sliding stops the sideways chase: well beyond the rounding at 8 km.
        let actual = Vec2::new(0.0, wanted.y);
        assert_eq!(
            movement.achieved_chase(actual, 8_000.0),
            (Vec2::ZERO, Attribution::Projected)
        );
    }

    #[test]
    fn stationary_prior_pair_cannot_consume_a_new_unsolved_request() {
        let mut requests = Requests::new();
        requests.complete(Vec3::ZERO, Vec3::ZERO);
        requests.submit(Movement::new(Vec2::X, -Vec2::X, 1.0));
        assert_eq!(requests.completed(), CompletedMovement::default());
        assert!(requests.pending());
        requests.complete(Vec3::ZERO, Vec3::ZERO);
        assert_eq!(requests.completed().current, -Vec2::X);
        assert!(!requests.pending());
        assert_eq!(
            requests.counts(),
            LedgerCounts {
                exact: 1,
                projected: 0
            }
        );
    }

    #[test]
    fn two_steps_before_render_accumulate_both_compensations() {
        let mut requests = Requests::new();
        for step in 0..2 {
            requests.submit(Movement::new(Vec2::ZERO, Vec2::X, 1.0));
            requests.complete(Vec3::X * step as f32, Vec3::X * (step + 1) as f32);
        }
        assert_eq!(
            requests.completed(),
            CompletedMovement {
                previous: Vec2::X,
                current: Vec2::X * 2.0
            }
        );
    }

    #[test]
    fn task_exit_and_stale_reset_settle_without_replaying() {
        let mut requests = Requests::new();
        requests.submit(Movement::new(Vec2::ZERO, Vec2::X, 1.0));
        requests.complete(Vec3::ZERO, Vec3::X);
        requests.complete(Vec3::X, Vec3::X + Vec3::Z);
        assert_eq!(
            requests.completed(),
            CompletedMovement {
                previous: Vec2::X,
                current: Vec2::X
            }
        );
        requests.submit(Movement::new(Vec2::ZERO, Vec2::Y, 1.0));
        requests.clear();
        requests.complete(Vec3::ZERO, Vec3::Z);
        assert_eq!(requests.completed().current, Vec2::X);
        assert_eq!(requests.completed().previous, Vec2::X);
    }

    #[test]
    fn a_wall_obstructs_and_a_shallow_slide_does_not() {
        let mut requests = Requests::new();
        for _ in 0..2 {
            requests.submit(Movement::new(Vec2::ZERO, Vec2::X * 0.1, 1.0));
            requests.complete(Vec3::ZERO, Vec3::ZERO);
        }
        assert!(requests.obstructed());
        // Along a wall at thirty degrees: most of the chase still goes through.
        let along = Vec2::new(30f32.to_radians().cos(), 30f32.to_radians().sin()) * 0.1;
        for _ in 0..2 {
            requests.submit(Movement::new(Vec2::ZERO, along, 1.0));
            requests.complete(
                Vec3::ZERO,
                Vec3::new(0.1 * 30f32.to_radians().cos(), 0.0, 0.0),
            );
        }
        assert!(!requests.obstructed());
        requests.submit(Movement::new(Vec2::ZERO, Vec2::X, 1.0));
        requests.complete(Vec3::ZERO, Vec3::ZERO);
        requests.clear();
        assert!(!requests.obstructed());
        // A chase that asks for nothing is never obstructed.
        requests.complete(Vec3::ZERO, Vec3::ZERO);
        assert!(!requests.obstructed());
    }

    #[test]
    fn a_wall_counts_as_projected() {
        let mut requests = Requests::new();
        requests.submit(Movement::new(Vec2::ZERO, Vec2::X, 1.0));
        requests.complete(Vec3::ZERO, Vec3::ZERO);
        assert_eq!(requests.completed().current, Vec2::ZERO);
        assert_eq!(
            requests.counts(),
            LedgerCounts {
                exact: 0,
                projected: 1
            }
        );
    }
}
