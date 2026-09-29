//! The cockpit shift: how far the view's cockpit origin has been moved to keep the camera on the
//! physical head while the body catches up with it. Pure.
//!
//! The camera is `anchor + body × (cockpit × scale)`, and the anchor is the animated head bone. When
//! the chase walks the body toward the head, the anchor moves with the body while the cockpit
//! offset stays, so without a correction the camera runs ahead by the distance the body covered and
//! the measured offset never shrinks: the body would chase the head forever. The shift takes the
//! chase's share of each tick's body displacement back out of the cockpit, so the camera stays with
//! the real head and the offset closes.
//!
//! The shift is kept as a tick-spaced pair, like the anchor it compensates: the view composes the
//! previous tick's side with the previous shift and the current side with the current one, so that
//! the engine's sub-tick interpolation moves the anchor and the shift together and the camera holds
//! still between ticks.

use glam::{Quat, Vec3};

/// The shift pair and the tick it was last advanced on.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct CockpitShift {
    prev: Vec3,
    cur: Vec3,
    /// The anchor pair of the last tick folded in, which identifies the tick.
    last_tick: Option<(Vec3, Vec3)>,
}

/// One frame's inputs to [`CockpitShift::observe`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TickAnchors {
    /// The animated head anchor this tick and the previous tick.
    pub anchor: Vec3,
    pub anchor_prev: Vec3,
    /// The body rotation the cockpit composes with.
    pub body: Quat,
    pub world_scale: f32,
}

impl CockpitShift {
    pub const fn new() -> Self {
        Self {
            prev: Vec3::ZERO,
            cur: Vec3::ZERO,
            last_tick: None,
        }
    }

    /// The shift for the current tick's side of the pose pair, cockpit frame.
    pub fn current(&self) -> Vec3 {
        self.cur
    }

    /// The raw cockpit position with the shift taken out, for the previous and current sides of the
    /// pose pair.
    pub fn apply(&self, raw: Vec3) -> (Vec3, Vec3) {
        (raw - self.prev, raw - self.cur)
    }

    /// Fold a rendered frame in. On the first frame of a new tick (a new anchor pair), advance the
    /// pair and take `share()` of the tick's horizontal body displacement out of the cockpit. The
    /// share is asked for only on a new tick, so a caller that consumes it once per tick sees each
    /// tick's share exactly once.
    pub fn observe(&mut self, frame: &TickAnchors, share: impl FnOnce() -> f32) {
        let key = (frame.anchor, frame.anchor_prev);
        if self.last_tick == Some(key) {
            return;
        }
        self.last_tick = Some(key);
        self.prev = self.cur;
        let share = share();
        let scale = frame.world_scale;
        if !(share > 0.0 && share.is_finite() && scale.is_finite() && scale > 0.0) {
            return;
        }
        let moved = frame.anchor - frame.anchor_prev;
        let moved = Vec3::new(moved.x, 0.0, moved.z) * share.min(1.0);
        let cockpit = frame.body.inverse() * moved / scale;
        if cockpit.is_finite() {
            self.cur += Vec3::new(cockpit.x, 0.0, cockpit.z);
        }
    }

    /// Re-base the horizontal offset: the shift takes out all of `raw`'s horizontal component, so
    /// that the head sits over the body, with no interpolation from the previous shift. For a
    /// teleport, which moves the body to a new place and must not leave the head where the old
    /// offset put it.
    pub fn rebase_horizontal(&mut self, raw: Vec3) {
        self.cur = Vec3::new(raw.x, self.cur.y, raw.z);
        self.prev = self.cur;
    }

    /// Clear the shift, when a recenter re-bases the baseline's position.
    pub fn clear(&mut self) {
        self.cur = Vec3::ZERO;
        self.prev = Vec3::ZERO;
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
        self.prev = self.cur;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(anchor: Vec3, anchor_prev: Vec3) -> TickAnchors {
        TickAnchors {
            anchor,
            anchor_prev,
            body: Quat::IDENTITY,
            world_scale: 1.0,
        }
    }

    #[test]
    fn the_chase_share_of_a_step_leaves_the_cockpit() {
        let mut shift = CockpitShift::default();
        shift.observe(
            &frame(Vec3::new(0.1, 1.7, 0.0), Vec3::new(0.0, 1.7, 0.0)),
            || 1.0,
        );
        let (prev, cur) = shift.apply(Vec3::new(0.3, 0.0, 0.0));
        assert!((prev - Vec3::new(0.3, 0.0, 0.0)).length() < 1e-6);
        assert!((cur - Vec3::new(0.2, 0.0, 0.0)).length() < 1e-6);
    }

    #[test]
    fn the_camera_holds_still_across_the_tick() {
        // The body steps 0.1 m toward a head 0.3 m ahead; the camera, anchor plus cockpit, is the
        // same on both sides of the pair, so the engine's interpolation cannot move it.
        let mut shift = CockpitShift::default();
        let (a0, a1) = (Vec3::ZERO, Vec3::new(0.1, 0.0, 0.0));
        shift.observe(&frame(a1, a0), || 1.0);
        let raw = Vec3::new(0.3, 0.0, 0.0);
        let (prev, cur) = shift.apply(raw);
        assert!(((a0 + prev) - (a1 + cur)).length() < 1e-6);
    }

    #[test]
    fn a_tick_is_folded_in_once() {
        let mut shift = CockpitShift::default();
        let f = frame(Vec3::new(0.1, 0.0, 0.0), Vec3::ZERO);
        let mut asked = 0;
        for _ in 0..3 {
            shift.observe(&f, || {
                asked += 1;
                1.0
            });
        }
        assert_eq!(asked, 1);
        assert!((shift.current() - Vec3::new(0.1, 0.0, 0.0)).length() < 1e-6);
    }

    #[test]
    fn stick_walking_is_not_taken_out() {
        let mut shift = CockpitShift::default();
        shift.observe(&frame(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO), || 0.0);
        assert_eq!(shift.current(), Vec3::ZERO);
    }

    #[test]
    fn vertical_motion_and_the_body_rotation_are_respected() {
        let mut shift = CockpitShift::default();
        shift.observe(
            &TickAnchors {
                anchor: Vec3::new(0.0, 0.5, -0.2),
                anchor_prev: Vec3::ZERO,
                // A quarter turn: the body's forward (-Z) is world -X.
                body: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                world_scale: 2.0,
            },
            || 1.0,
        );
        // World -Z is the body's right (+X) after the turn, halved by the world scale.
        assert!((shift.current() - Vec3::new(0.1, 0.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn a_teleport_re_bases_without_interpolating() {
        let mut shift = CockpitShift::default();
        shift.rebase_horizontal(Vec3::new(0.4, 0.2, -0.3));
        let (prev, cur) = shift.apply(Vec3::new(0.4, 0.2, -0.3));
        assert_eq!(prev, cur);
        assert!((cur - Vec3::new(0.0, 0.2, 0.0)).length() < 1e-6);
        shift.clear();
        assert_eq!(shift.current(), Vec3::ZERO);
    }

    #[test]
    fn a_yaw_recenter_turns_the_shift_with_the_frame() {
        let mut shift = CockpitShift::default();
        shift.rebase_horizontal(Vec3::new(1.0, 0.0, 0.0));
        shift.rotate(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        assert!((shift.current() - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-5);
    }
}
