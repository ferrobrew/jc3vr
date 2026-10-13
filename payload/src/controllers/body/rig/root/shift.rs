//! The cockpit shift: how far the view's cockpit origin has been moved to keep the camera on the
//! physical head while the body catches up with it. Pure.
//!
//! The camera is `anchor + body × (cockpit × scale)`, and the anchor rides the root. When the chase
//! walks the root toward the head, the anchor moves with the root while the cockpit offset stays,
//! so without a correction the camera runs ahead by the distance the root covered and the gap never
//! shrinks: the root would chase the head forever. The shift takes the chase's part of the root's
//! displacement back out of the cockpit, so the camera stays with the real head and the gap closes.
//!
//! The ledger ([`crate::controllers::body::rig::root::ledger`]) attributes each completed step's
//! displacement to the chase and keeps the running total at the last two completed steps
//! ([`CompletedMovement`]). The shift folds in everything completed since it last looked, so
//! several steps between rendered frames all count, and rendering never consumes a request.
//! Animated head motion is not attributed to locomotion.
//!
//! The shift is kept as a step-spaced pair, like the anchor it compensates: the view composes the
//! previous step's side with the previous shift and the current side with the current one, so that
//! the engine's sub-step interpolation moves the anchor and the shift together and the camera holds
//! still between steps.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::rig::root::ledger::CompletedMovement;

/// The shift pair, and the cumulative completed movement it has folded in.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct CockpitShift {
    prev: Vec3,
    cur: Vec3,
    observed: Vec2,
}

/// The cockpit frame in which the world-space correction is expressed.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CockpitBasis {
    /// The body rotation the cockpit composes with.
    pub body: Quat,
    /// World metres per cockpit metre.
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

    /// The shift for the current step's side of the pose pair, cockpit frame.
    pub fn current(&self) -> Vec3 {
        self.cur
    }

    /// The cumulative completed chase movement the shift has folded in, world `(x, z)`.
    pub fn observed(&self) -> Vec2 {
        self.observed
    }

    /// The raw cockpit position with the shift taken out, for the previous and current sides of the
    /// pose pair.
    pub fn apply(&self, raw: Vec3) -> (Vec3, Vec3) {
        (raw - self.prev, raw - self.cur)
    }

    /// Apply every completed step since the last observation, then reconstruct the latest pair.
    pub fn observe(&mut self, frame: &CockpitBasis, movement: CompletedMovement) {
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

    /// Collapse the pair onto the current shift, so the view does not interpolate across a jump.
    pub fn settle(&mut self) {
        self.prev = self.cur;
    }

    /// Re-base the horizontal offset: the shift takes out all of `raw`'s horizontal component, so
    /// that the head sits over the body, with no interpolation from the previous shift. For a
    /// teleport, which moves the body to a new place and must not leave the head where the old
    /// offset put it.
    pub fn rebase_horizontal(&mut self, raw: Vec3) {
        self.cur = Vec3::new(raw.x, self.cur.y, raw.z);
        self.settle();
    }

    /// Move the view by `offset`, cockpit frame, on both sides of the pair alike, so the view does
    /// not interpolate across it: the push-back holding the view at a wall.
    pub fn slide_view(&mut self, offset: Vec3) {
        if offset.is_finite() {
            self.prev -= offset;
            self.cur -= offset;
        }
    }

    /// Clear the shift, when a recenter re-bases the baseline's position.
    pub fn clear(&mut self) {
        self.cur = Vec3::ZERO;
        self.settle();
    }

    /// Carry the shift into a cockpit frame turned by `rotation` (new frame from old), when a
    /// recenter re-bases only the baseline's yaw and height: the shift is a fixed offset in the room,
    /// so it turns with the frame it is expressed in.
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

    fn frame() -> CockpitBasis {
        CockpitBasis {
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
    fn a_slide_moves_both_sides_of_the_pair() {
        let mut shift = CockpitShift::new();
        shift.observe(
            &frame(),
            CompletedMovement {
                previous: Vec2::ZERO,
                current: Vec2::X * 0.1,
            },
        );
        shift.slide_view(Vec3::Z * 0.2);
        assert_eq!(
            shift.apply(Vec3::ZERO),
            (Vec3::Z * 0.2, Vec3::X * -0.1 + Vec3::Z * 0.2)
        );
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
