//! Drawing a hand where its body is, pure: the offset from where the controller would have the
//! palm to where the hand's body is, and that offset carried onto the hand's cockpit poses.
//!
//! The offset is measured against the controller's palm, not against the goal the body was driven
//! to, so a hand whose body is held elsewhere on purpose (a grip pinning it to a wall) is drawn
//! there too. It has a turn as well as a shift, so a wrist twisted into a wall is drawn turned as
//! its body is, not through the wall; the turn is carried about the palm's centre.
//!
//! The offset is measured once per physics step, but the hand is drawn every rendered frame from
//! the latest tracking. Carried as one step's constant, a hand pressed into a wall sinks into it by
//! the controller's travel since the step and springs back out at the next, a visible stutter at
//! the step rate. So the offset drawn at render time is the last two steps' offsets blended by the
//! time since the latest step over the interval between them ([`OffsetTimeline`]): it arrives at
//! the latest step's offset as the next step lands, so it is continuous across the steps, one step
//! behind them.

use std::time::{Duration, Instant};

use glam::{Quat, Vec3};

use crate::controllers::{frame::Pose, hands::CockpitHand};

/// The offset that draws a hand where its body is.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RenderOffset {
    /// The palm centre's shift.
    pub shift: Vec3,
    /// The turn about the palm's centre.
    pub turn: Quat,
}

impl RenderOffset {
    pub const NONE: Self = Self {
        shift: Vec3::ZERO,
        turn: Quat::IDENTITY,
    };

    /// The offset from the controller's palm at `palm` to the body at `body`, world space. The
    /// shift is capped at `max_shift_m`; a turn within `deadband_rad` is not drawn, so a free hand
    /// keeps the controller's own rotation, and past it only the excess is, so the drawn turn
    /// grows from nothing; the drawn turn is capped at `max_turn_rad`.
    pub fn between(
        palm: Pose,
        body: Pose,
        max_shift_m: f32,
        deadband_rad: f32,
        max_turn_rad: f32,
    ) -> Self {
        let shift = body.position - palm.position;
        if !(shift.is_finite() && palm.orientation.is_finite() && body.orientation.is_finite()) {
            return Self::NONE;
        }
        let turn = body.orientation.normalize() * palm.orientation.normalize().inverse();
        let turn = if turn.w < 0.0 { -turn } else { turn };
        let (axis, angle) = turn.to_axis_angle();
        let drawn = (angle - deadband_rad.max(0.0)).clamp(0.0, max_turn_rad.max(0.0));
        let turn = if drawn > 0.0 && axis.is_finite() {
            Quat::from_axis_angle(axis, drawn)
        } else {
            Quat::IDENTITY
        };
        Self {
            shift: shift.clamp_length_max(max_shift_m.max(0.0)),
            turn,
        }
    }

    /// This offset, world space, in the cockpit frame of a body facing `body_rotation` at the VR
    /// world scale `world_scale`.
    pub fn to_cockpit(self, body_rotation: Quat, world_scale: f32) -> Self {
        let inverse = body_rotation.inverse();
        Self {
            shift: inverse * self.shift / world_scale.max(1e-3),
            turn: (inverse * self.turn * body_rotation).normalize(),
        }
    }

    pub fn is_none(&self) -> bool {
        self.shift == Vec3::ZERO && self.turn == Quat::IDENTITY
    }

    /// Carry `hand`'s cockpit poses by this cockpit-frame offset, turning them about the palm's
    /// centre, which sits at `palm_offset` in the wrist frame.
    pub fn apply(&self, hand: &mut CockpitHand, palm_offset: Vec3) {
        let pivot = hand.wrist.position + hand.wrist.orientation * palm_offset;
        for pose in [&mut hand.wrist, &mut hand.grip, &mut hand.aim] {
            self.carry(pose, pivot);
        }
    }

    fn carry(&self, pose: &mut Pose, pivot: Vec3) {
        pose.position = pivot + self.turn * (pose.position - pivot) + self.shift;
        pose.orientation = (self.turn * pose.orientation).normalize();
    }

    /// The offset `t` of the way from `self` to `to`: the shift lerped and the turn slerped.
    pub fn lerp(self, to: Self, t: f32) -> Self {
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            1.0
        };
        Self {
            shift: self.shift.lerp(to.shift, t),
            turn: self.turn.slerp(to.turn, t).normalize(),
        }
    }
}

/// The last two physics steps' offsets and when each landed, for drawing the offset between them at
/// render time.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct OffsetTimeline {
    previous: Option<Step>,
    current: Option<Step>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Step {
    offsets: [RenderOffset; 2],
    at: Instant,
}

impl OffsetTimeline {
    pub const EMPTY: Self = Self {
        previous: None,
        current: None,
    };

    /// Record a step's offsets, landed at `at`.
    pub fn push(&mut self, offsets: [RenderOffset; 2], at: Instant) {
        self.previous = self.current;
        self.current = Some(Step { offsets, at });
    }

    /// Forget the steps, so the next one is drawn as it lands rather than blended from an offset
    /// that no longer applies.
    pub fn reset(&mut self) {
        *self = Self::EMPTY;
    }

    /// The offsets to draw at `now`: the previous step's blended toward the latest one's by the
    /// time since the latest step over the interval between the two ([`blend_fraction`]), or the
    /// latest step's alone when there is no previous one. `None` before any step.
    pub fn at(&self, now: Instant) -> Option<[RenderOffset; 2]> {
        let current = self.current?;
        let Some(previous) = self.previous else {
            return Some(current.offsets);
        };
        let t = blend_fraction(
            now.saturating_duration_since(current.at),
            current.at.saturating_duration_since(previous.at),
        );
        Some(std::array::from_fn(|i| {
            previous.offsets[i].lerp(current.offsets[i], t)
        }))
    }
}

/// How far from the previous step's offset toward the latest one's to draw, `since` after the
/// latest step, which came `interval` after the previous one: the share of the interval that has
/// passed, held at one once it has. An interval that is empty or longer than [`MAX_BLEND_INTERVAL`]
/// (a stall, or a hand that stopped acting for a while) is not blended across.
pub fn blend_fraction(since: Duration, interval: Duration) -> f32 {
    if interval.is_zero() || interval > MAX_BLEND_INTERVAL {
        return 1.0;
    }
    (since.as_secs_f32() / interval.as_secs_f32()).clamp(0.0, 1.0)
}

/// The longest interval between two steps that is blended across: a few ticks.
pub const MAX_BLEND_INTERVAL: Duration = Duration::from_millis(150);

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(position: Vec3, orientation: Quat) -> Pose {
        Pose {
            position,
            orientation,
        }
    }

    #[test]
    fn a_hand_held_at_a_wall_is_drawn_at_its_body() {
        let palm = pose(Vec3::new(0.0, 1.0, -0.6), Quat::IDENTITY);
        let body = pose(Vec3::new(0.0, 1.0, -0.4), Quat::IDENTITY);
        let offset = RenderOffset::between(palm, body, 0.6, 0.05, 1.5);
        assert!(offset.shift.abs_diff_eq(Vec3::Z * 0.2, 1e-6));
        assert_eq!(offset.turn, Quat::IDENTITY);
        // Capped.
        let far = RenderOffset::between(palm, pose(Vec3::ZERO, Quat::IDENTITY), 0.3, 0.05, 1.5);
        assert!((far.shift.length() - 0.3).abs() < 1e-5);
    }

    #[test]
    fn a_twisted_body_turns_the_drawn_hand_past_the_deadband() {
        let palm = pose(Vec3::ZERO, Quat::IDENTITY);
        let small = RenderOffset::between(
            palm,
            pose(Vec3::ZERO, Quat::from_rotation_y(0.03)),
            1.0,
            0.05,
            1.5,
        );
        assert!(small.is_none());
        let twisted = RenderOffset::between(
            palm,
            pose(Vec3::ZERO, Quat::from_rotation_y(0.55)),
            1.0,
            0.05,
            1.5,
        );
        assert!(twisted.turn.angle_between(Quat::from_rotation_y(0.5)) < 1e-4);
        let capped = RenderOffset::between(
            palm,
            pose(Vec3::ZERO, Quat::from_rotation_y(2.5)),
            1.0,
            0.05,
            1.0,
        );
        assert!((capped.turn.angle_between(Quat::IDENTITY) - 1.0).abs() < 1e-4);
        let bad = RenderOffset::between(palm, pose(Vec3::NAN, Quat::IDENTITY), 1.0, 0.05, 1.0);
        assert!(bad.is_none());
    }

    #[test]
    fn the_turn_is_carried_about_the_palm_centre() {
        let mut hand = CockpitHand {
            wrist: pose(Vec3::ZERO, Quat::IDENTITY),
            grip: pose(Vec3::new(0.0, 0.0, -0.05), Quat::IDENTITY),
            aim: pose(Vec3::new(0.0, 0.0, -0.1), Quat::IDENTITY),
        };
        let palm_offset = Vec3::new(0.0, 0.0, -0.06);
        let offset = RenderOffset {
            shift: Vec3::X * 0.1,
            turn: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        offset.apply(&mut hand, palm_offset);
        // The palm centre only shifts.
        let pivot = hand.wrist.position + hand.wrist.orientation * palm_offset;
        assert!(pivot.abs_diff_eq(Vec3::new(0.1, 0.0, -0.06), 1e-5));
        assert!(hand.aim.orientation.angle_between(offset.turn) < 1e-5);
    }

    #[test]
    fn the_blend_runs_across_the_interval_and_holds_past_it() {
        let interval = Duration::from_millis(33);
        assert_eq!(blend_fraction(Duration::ZERO, interval), 0.0);
        let half = blend_fraction(Duration::from_micros(16_500), interval);
        assert!((half - 0.5).abs() < 1e-4);
        assert_eq!(blend_fraction(Duration::from_millis(50), interval), 1.0);
        assert_eq!(blend_fraction(Duration::ZERO, Duration::ZERO), 1.0);
        assert_eq!(blend_fraction(Duration::ZERO, Duration::from_secs(1)), 1.0);
    }

    fn shifted(x: f32) -> [RenderOffset; 2] {
        [
            RenderOffset {
                shift: Vec3::X * x,
                turn: Quat::IDENTITY,
            },
            RenderOffset::NONE,
        ]
    }

    #[test]
    fn a_hand_pressed_into_a_wall_is_drawn_without_a_step_between_physics_steps() {
        // The controller pushes on into a wall at a steady pace: at each step the body is held
        // back a centimetre further from its palm.
        let start = Instant::now();
        let tick = Duration::from_millis(33);
        let mut timeline = OffsetTimeline::EMPTY;
        let mut drawn = Vec::new();
        for step in 0..6u32 {
            let at = start + tick * step;
            timeline.push(shifted(0.01 * step as f32), at);
            // The frames between this step and the next, at about 90 Hz, and the last moment
            // before the next step lands.
            for frame in 0..3u32 {
                drawn.push(
                    timeline.at(at + Duration::from_millis(11) * frame).unwrap()[0]
                        .shift
                        .x,
                );
            }
            drawn.push(timeline.at(at + tick).unwrap()[0].shift.x);
        }
        // Monotonic, and no frame moves by more than a frame's share of a step's change.
        for pair in drawn.windows(2) {
            assert!(pair[1] >= pair[0] - 1e-6, "{drawn:?}");
            assert!(pair[1] - pair[0] <= 0.01 * 0.34 + 1e-5, "{drawn:?}");
        }
    }

    #[test]
    fn a_lone_step_is_drawn_as_it_landed_and_a_reset_forgets_it() {
        let mut timeline = OffsetTimeline::EMPTY;
        let now = Instant::now();
        assert!(timeline.at(now).is_none());
        timeline.push(shifted(0.02), now);
        assert_eq!(timeline.at(now).unwrap(), shifted(0.02));
        timeline.reset();
        assert!(timeline.at(now).is_none());
    }

    #[test]
    fn the_lerp_reaches_both_ends() {
        let a = RenderOffset {
            shift: Vec3::X * 0.1,
            turn: Quat::from_rotation_y(0.2),
        };
        let b = RenderOffset {
            shift: Vec3::Z * 0.1,
            turn: Quat::from_rotation_y(0.6),
        };
        assert!(a.lerp(b, 0.0).shift.abs_diff_eq(a.shift, 1e-6));
        assert!(a.lerp(b, 1.0).turn.angle_between(b.turn) < 1e-4);
        assert!(
            a.lerp(b, 0.5)
                .turn
                .angle_between(Quat::from_rotation_y(0.4))
                < 1e-4
        );
    }

    #[test]
    fn the_cockpit_offset_undoes_the_body_facing_and_scale() {
        let body = Quat::from_rotation_y(1.0);
        let world = RenderOffset {
            shift: body * Vec3::X * 0.2,
            turn: body * Quat::from_rotation_x(0.3) * body.inverse(),
        };
        let cockpit = world.to_cockpit(body, 2.0);
        assert!(cockpit.shift.abs_diff_eq(Vec3::X * 0.1, 1e-5));
        assert!(cockpit.turn.angle_between(Quat::from_rotation_x(0.3)) < 1e-4);
    }
}
