//! Drawing a hand where its body is, pure: the offset from where the controller would have the
//! palm to where the hand's body is, and that offset carried onto the hand's cockpit poses.
//!
//! The offset is measured against the controller's palm, not against the goal the body was driven
//! to, so a hand whose body is held elsewhere on purpose (a grip pinning it to a wall) is drawn
//! there too. It has a turn as well as a shift, so a wrist twisted into a wall is drawn turned as
//! its body is, not through the wall; the turn is carried about the palm's centre.

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
}

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
