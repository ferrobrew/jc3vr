//! The cockpit shift consumes cumulative compensation published at completed simulation steps.
//! Animated head motion is not attributed to locomotion, and rendering never consumes a request.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::roomscale::sim::CompletedMovement;

#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct CockpitShift {
    prev: Vec3,
    cur: Vec3,
    observed: Vec2,
}

/// The cockpit frame in which the world-space correction is expressed.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TickAnchors {
    pub body: Quat,
    pub world_scale: f32,
}

impl CockpitShift {
    pub const fn new() -> Self {
        Self {
            prev: Vec3::ZERO,
            cur: Vec3::ZERO,
            observed: Vec2::ZERO,
        }
    }

    pub fn current(&self) -> Vec3 {
        self.cur
    }

    pub fn apply(&self, raw: Vec3) -> (Vec3, Vec3) {
        (raw - self.prev, raw - self.cur)
    }

    /// Apply every completed step since the last observation, then reconstruct the latest pair.
    pub fn observe(&mut self, frame: &TickAnchors, movement: CompletedMovement) {
        let scale = frame.world_scale;
        if !scale.is_finite() || scale <= 0.0 || !frame.body.is_finite() {
            self.settle();
            return;
        }
        let to_cockpit = |moved: Vec2| {
            let v = frame.body.inverse() * Vec3::new(moved.x, 0.0, moved.y) / scale;
            Vec3::new(v.x, 0.0, v.z)
        };
        self.cur += to_cockpit(movement.current - self.observed);
        self.prev = self.cur - to_cockpit(movement.current - movement.previous);
        self.observed = movement.current;
    }

    /// Discard completed compensation when rebasing or disabling, without losing its baseline.
    pub fn synchronize(&mut self, movement: CompletedMovement) {
        self.observed = movement.current;
        self.settle();
    }

    pub fn settle(&mut self) {
        self.prev = self.cur;
    }

    pub fn rebase_horizontal(&mut self, raw: Vec3) {
        self.cur = Vec3::new(raw.x, self.cur.y, raw.z);
        self.settle();
    }

    pub fn clear(&mut self) {
        self.cur = Vec3::ZERO;
        self.settle();
    }

    /// Carry the offset into the yaw-recentered cockpit frame.
    pub fn rotate(&mut self, rotation: Quat) {
        let turned = rotation * Vec3::new(self.cur.x, 0.0, self.cur.z);
        self.cur = if turned.is_finite() {
            turned
        } else {
            Vec3::ZERO
        };
        self.settle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> TickAnchors {
        TickAnchors {
            body: Quat::IDENTITY,
            world_scale: 1.0,
        }
    }

    #[test]
    fn multiple_completed_steps_before_render_preserve_the_latest_pair() {
        let mut shift = CockpitShift::new();
        let completed = CompletedMovement {
            previous: Vec2::X,
            current: Vec2::X * 2.0,
        };
        shift.observe(&frame(), completed);
        assert_eq!(shift.apply(Vec3::ZERO), (-Vec3::X, -Vec3::X * 2.0));
        shift.observe(&frame(), completed);
        assert_eq!(shift.apply(Vec3::ZERO), (-Vec3::X, -Vec3::X * 2.0));
    }

    #[test]
    fn interpolation_preserves_the_task_vector() {
        let mut shift = CockpitShift::new();
        shift.observe(
            &frame(),
            CompletedMovement {
                previous: Vec2::ZERO,
                current: Vec2::X * 0.1,
            },
        );
        let raw = Vec3::X * 0.3;
        let (prev, cur) = shift.apply(raw);
        for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let camera = prev.lerp(Vec3::X * 0.1 + Vec3::Z * 0.2 + cur, alpha);
            assert!(camera.distance(raw + Vec3::Z * 0.2 * alpha) < 1e-6);
        }
    }

    #[test]
    fn task_exit_and_staleness_settle_the_pair() {
        let mut shift = CockpitShift::new();
        shift.observe(
            &frame(),
            CompletedMovement {
                previous: Vec2::ZERO,
                current: Vec2::X,
            },
        );
        shift.observe(
            &frame(),
            CompletedMovement {
                previous: Vec2::X,
                current: Vec2::X,
            },
        );
        assert_eq!(shift.apply(Vec3::ZERO), (-Vec3::X, -Vec3::X));
    }

    #[test]
    fn disable_and_recenter_do_not_replay_completed_motion() {
        let mut shift = CockpitShift::new();
        let completed = CompletedMovement {
            previous: Vec2::X,
            current: Vec2::X,
        };
        shift.observe(&frame(), completed);
        shift.synchronize(completed);
        shift.rotate(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        assert!(shift.current().distance(-Vec3::Z) < 1e-5);
        shift.clear();
        shift.observe(&frame(), completed);
        assert_eq!(shift.apply(Vec3::ZERO), (Vec3::ZERO, Vec3::ZERO));
    }
}
