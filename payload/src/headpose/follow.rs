//! The body turning under the view: the character's facing follows the player's torso on foot
//! while the view stays where the player looks. Pure.
//!
//! Under the VR source the view composes the head onto the character's body frame (`body ×
//! cockpit`, see [`crate::headpose::xr`]), so turning the character turns the view. To let the
//! body follow the player's estimated torso without turning the view, the view frame is the body
//! frame turned back by an offset about the body's up axis: `view = body × Ry(-offset)`. The
//! offset is the body's heading relative to the view, and the view's own heading is held apart:
//!
//! - **Following** (on foot, with the setting on): the view's heading is the turn accumulator's
//!   heading, which only the turn input (the stick, the mouse, a snap step) moves, kept as a
//!   tick-spaced pair. The body is steered to the view's heading plus the torso's heading in the
//!   cockpit frame, so it faces the way the player's torso does. The offset is whatever the body's
//!   real heading is minus the view's, read from the same tick-spaced body transforms the view
//!   interpolates between, so however the game turns the body, at whatever rate, none of it reaches
//!   the view.
//! - **Not following** (off foot, swimming, or the setting off): the offset is frozen at its last
//!   value, so the view rides the body as it always has, turned by a constant. The body is steered
//!   to the accumulator's heading plus the frozen offset, which leaves the view converging on the
//!   accumulator's heading exactly as it did before the offset existed.
//!
//! A recenter clears the offset: it re-bases the cockpit to the head, which already turns the view.
//! Once the body stops following, the frozen offset is folded into the cockpit baseline at the top
//! of the next frame ([`BodyFollow::bank`]): the baseline turns with the body and the offset goes to
//! zero together, so the view does not move and the cockpit frame's forward is the body's again,
//! as everything off foot (the vehicles' controls, the driving view, the flight steering) expects.
//!
//! Headings are world yaws about +Y in radians, as [`crate::headpose::sim::body_yaw_of`] reads them.

use crate::headpose::sim::wrap_angle;

/// The follow state, owned by the VR body-yaw accumulator.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct BodyFollow {
    /// The view's heading at the previous and the current tick, while following.
    view: Option<(f32, f32)>,
    /// The body's target heading relative to the view while following: the torso's heading,
    /// through the deadband.
    target: f32,
    /// The offset while not following.
    frozen: f32,
}

/// One input tick's inputs to [`BodyFollow::tick`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FollowTick {
    /// Whether the body follows this tick.
    pub follow: bool,
    /// The turn accumulator's heading after this tick's input.
    pub heading: f32,
    /// The body's current heading.
    pub body: f32,
    /// The torso's heading in the cockpit frame, when estimated.
    pub torso: Option<f32>,
    /// How far the torso may turn from the body's target before the target moves, radians.
    pub deadband: f32,
}

impl BodyFollow {
    pub const fn new() -> Self {
        Self {
            view: None,
            target: 0.0,
            frozen: 0.0,
        }
    }

    /// Whether the body is following.
    pub fn following(&self) -> bool {
        self.view.is_some()
    }

    /// The offset a fresh turn accumulator seeds against: the view's heading is the body's minus
    /// it. Zero until the body has followed.
    pub fn frozen(&self) -> f32 {
        self.frozen
    }

    /// The body's target heading relative to the view: the torso's while following, the frozen
    /// offset otherwise.
    pub fn target(&self) -> f32 {
        if self.following() {
            self.target
        } else {
            self.frozen
        }
    }

    /// Advance one input tick.
    pub fn tick(&mut self, tick: FollowTick) {
        if !tick.follow || !tick.heading.is_finite() || !tick.body.is_finite() {
            if let Some((_, current)) = self.view.take() {
                self.frozen = wrap_angle(tick.body - current);
            }
            return;
        }
        let previous = match self.view {
            Some((_, current)) => current,
            None => {
                // The body holds its place relative to the view until the torso moves it.
                self.target = self.frozen;
                tick.heading
            }
        };
        self.view = Some((previous, tick.heading));
        if let Some(torso) = tick.torso.filter(|t| t.is_finite()) {
            let deadband = if tick.deadband.is_finite() {
                tick.deadband.max(0.0)
            } else {
                0.0
            };
            let off = wrap_angle(torso - self.target);
            if off.abs() > deadband {
                self.target = wrap_angle(torso - off.signum() * deadband);
            }
        }
    }

    /// The offset of a body whose heading is `body`, at the current tick (`current`) or the
    /// previous one.
    pub fn offset(&self, body: f32, current: bool) -> f32 {
        match self.view {
            Some((previous, now)) if body.is_finite() => {
                wrap_angle(body - if current { now } else { previous })
            }
            Some(_) => 0.0,
            None => self.frozen,
        }
    }

    /// Take `yaw` out of the frozen offset, for a cockpit baseline that turned by it: the view
    /// composes the same either way.
    pub fn bank(&mut self, yaw: f32) {
        if yaw.is_finite() {
            self.frozen = wrap_angle(self.frozen - yaw);
        }
    }

    /// Clear the offset, for a recenter: the view takes the body's heading `body` from here.
    pub fn clear(&mut self, body: f32) {
        self.frozen = 0.0;
        self.target = 0.0;
        if self.view.is_some() && body.is_finite() {
            self.view = Some((body, body));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deg(d: f32) -> f32 {
        d.to_radians()
    }

    fn tick(follow: bool, heading: f32, body: f32, torso: Option<f32>) -> FollowTick {
        FollowTick {
            follow,
            heading,
            body,
            torso,
            deadband: deg(5.0),
        }
    }

    /// The body heading a rate-limited executor reaches from `body` toward `target` in a tick.
    fn step(body: f32, target: f32, max_step: f32) -> f32 {
        wrap_angle(body + wrap_angle(target - body).clamp(-max_step, max_step))
    }

    #[test]
    fn the_view_stays_put_while_the_body_turns_to_the_torso() {
        let mut follow = BodyFollow::new();
        let view = deg(30.0);
        let mut body = view;
        let mut views = Vec::new();
        for _ in 0..40 {
            follow.tick(tick(true, view, body, Some(deg(90.0))));
            let previous_body = body;
            body = step(body, view + follow.target(), deg(10.0));
            // The view composes each side of the pair from that side's body.
            views.push((
                wrap_angle(previous_body - follow.offset(previous_body, false)),
                wrap_angle(body - follow.offset(body, true)),
            ));
        }
        // The body faces the torso's heading, short by the deadband.
        assert!((wrap_angle(body - view) - deg(85.0)).abs() < 1e-4);
        for (previous, current) in views {
            assert!((previous - view).abs() < 1e-5 && (current - view).abs() < 1e-5);
        }
    }

    #[test]
    fn the_turn_input_alone_turns_the_view() {
        let mut follow = BodyFollow::new();
        let mut body = 0.0;
        let mut heading = 0.0;
        for i in 0..20 {
            let previous_heading = heading;
            heading = wrap_angle(heading + deg(3.0));
            follow.tick(tick(true, heading, body, Some(0.0)));
            let previous_body = body;
            // The game also kicks the body by a few degrees now and then.
            body = step(body, heading + follow.target(), deg(10.0))
                + if i % 5 == 0 { 0.05 } else { 0.0 };
            if i > 0 {
                let previous = wrap_angle(previous_body - follow.offset(previous_body, false));
                assert!((previous - previous_heading).abs() < 1e-5);
            }
            let current = wrap_angle(body - follow.offset(body, true));
            assert!((current - heading).abs() < 1e-5);
        }
    }

    #[test]
    fn small_torso_motion_inside_the_deadband_leaves_the_target() {
        let mut follow = BodyFollow::new();
        follow.tick(tick(true, 0.0, 0.0, Some(deg(20.0))));
        assert!((follow.target() - deg(15.0)).abs() < 1e-5);
        follow.tick(tick(true, 0.0, 0.0, Some(deg(17.0))));
        assert!((follow.target() - deg(15.0)).abs() < 1e-5);
        follow.tick(tick(true, 0.0, 0.0, Some(deg(5.0))));
        assert!((follow.target() - deg(10.0)).abs() < 1e-5);
        // No estimate holds the target.
        follow.tick(tick(true, 0.0, 0.0, None));
        assert!((follow.target() - deg(10.0)).abs() < 1e-5);
    }

    #[test]
    fn stopping_freezes_the_offset_and_keeps_the_body_where_it_is() {
        let mut follow = BodyFollow::new();
        follow.tick(tick(true, deg(10.0), deg(50.0), Some(deg(40.0))));
        follow.tick(tick(false, deg(10.0), deg(50.0), Some(deg(40.0))));
        assert!(!follow.following());
        assert!((follow.offset(deg(80.0), true) - deg(40.0)).abs() < 1e-5);
        assert!((follow.target() - deg(40.0)).abs() < 1e-5);
        // A body steered to the heading plus the frozen offset does not move.
        assert!((wrap_angle(deg(10.0) + follow.target()) - deg(50.0)).abs() < 1e-5);
    }

    #[test]
    fn resuming_starts_from_the_frozen_offset() {
        let mut follow = BodyFollow::new();
        follow.tick(tick(true, 0.0, deg(30.0), Some(deg(30.0))));
        follow.tick(tick(false, 0.0, deg(30.0), None));
        // Off foot the body was turned by the game; the view rode it.
        let body = deg(100.0);
        let view = wrap_angle(body - follow.offset(body, true));
        // Back on foot the accumulator seeds at the view's heading.
        follow.tick(tick(true, view, body, None));
        assert!((wrap_angle(body - follow.offset(body, true)) - view).abs() < 1e-5);
        assert!((follow.target() - deg(30.0)).abs() < 1e-5);
    }

    #[test]
    fn a_recenter_clears_the_offset() {
        let mut follow = BodyFollow::new();
        follow.tick(tick(true, 0.0, deg(45.0), Some(deg(45.0))));
        follow.clear(deg(45.0));
        assert_eq!(follow.offset(deg(45.0), true), 0.0);
        assert_eq!(follow.offset(deg(45.0), false), 0.0);
        assert_eq!(follow.target(), 0.0);
        let mut stopped = BodyFollow::new();
        stopped.tick(tick(true, 0.0, deg(45.0), None));
        stopped.tick(tick(false, 0.0, deg(45.0), None));
        stopped.clear(deg(45.0));
        assert_eq!(stopped.offset(deg(90.0), true), 0.0);
    }

    #[test]
    fn banking_moves_the_frozen_offset_out() {
        let mut follow = BodyFollow::new();
        follow.tick(tick(true, 0.0, deg(30.0), None));
        follow.tick(tick(false, 0.0, deg(30.0), None));
        follow.bank(deg(30.0));
        assert!(follow.frozen().abs() < 1e-6);
        assert!(follow.offset(deg(80.0), true).abs() < 1e-6);
        follow.bank(f32::NAN);
        assert!(follow.frozen().abs() < 1e-6);
    }

    #[test]
    fn a_follow_that_never_ran_is_inert() {
        let follow = BodyFollow::new();
        assert_eq!(follow.offset(1.0, true), 0.0);
        assert_eq!(follow.target(), 0.0);
        assert!(!follow.following());
    }
}
