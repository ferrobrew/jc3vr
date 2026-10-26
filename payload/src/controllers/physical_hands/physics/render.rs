//! Presentation offsets measured between a published controller palm and its post-step body.
//!
//! Offsets are expressed in the palm publication's transport frame before the render head pair
//! carries them into the world. They are not interpolated across physics steps: doing so mixes
//! an old collision response with the current tracking and transport frame.

use std::time::Instant;

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

    /// Measure the body against the palm at the same sample time, in world space.
    /// The shift and turn are capped; a small free-hand turn stays inside the deadband.
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

    /// Express the world offset in the transport frame of the published palm.
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

    /// Carry the hand's poses about the palm centre, which sits in the wrist frame.
    pub fn apply(&self, hand: &mut CockpitHand, palm_offset: Vec3) {
        let pivot = hand.wrist.position + hand.wrist.orientation * palm_offset;
        for pose in [&mut hand.wrist, &mut hand.grip, &mut hand.aim] {
            pose.position = pivot + self.turn * (pose.position - pivot) + self.shift;
            pose.orientation = (self.turn * pose.orientation).normalize();
        }
    }
}

/// The latest pre-step offset, retained as a fallback when a post-step body read fails.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct OffsetTimeline {
    current: Option<([RenderOffset; 2], Instant)>,
}

impl OffsetTimeline {
    pub const EMPTY: Self = Self { current: None };

    pub fn push(&mut self, offsets: [RenderOffset; 2], at: Instant) {
        self.current = Some((offsets, at));
    }

    pub fn reset(&mut self) {
        *self = Self::EMPTY;
    }

    /// Keep the latest result instead of introducing another step of presentation delay.
    pub fn at(&self, _now: Instant) -> Option<[RenderOffset; 2]> {
        self.current.map(|(offsets, _)| offsets)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn pose(position: Vec3, orientation: Quat) -> Pose {
        Pose {
            position,
            orientation,
        }
    }

    #[test]
    fn a_hand_at_a_wall_keeps_its_collision_offset() {
        let palm = pose(Vec3::new(0.0, 1.0, -0.6), Quat::IDENTITY);
        let body = pose(Vec3::new(0.0, 1.0, -0.4), Quat::IDENTITY);
        let offset = RenderOffset::between(palm, body, 0.6, 0.05, 1.5);
        assert!(offset.shift.abs_diff_eq(Vec3::Z * 0.2, 1e-6));
        assert_eq!(offset.turn, Quat::IDENTITY);
        let far = RenderOffset::between(palm, pose(Vec3::ZERO, Quat::IDENTITY), 0.3, 0.05, 1.5);
        assert!((far.shift.length() - 0.3).abs() < 1e-5);
    }

    #[test]
    fn a_twisted_body_preserves_the_turn_deadband_and_cap() {
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
        assert!(
            RenderOffset::between(palm, pose(Vec3::NAN, Quat::IDENTITY), 1.0, 0.05, 1.0).is_none()
        );
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
        let pivot = hand.wrist.position + hand.wrist.orientation * palm_offset;
        assert!(pivot.abs_diff_eq(Vec3::new(0.1, 0.0, -0.06), 1e-5));
        assert!(hand.aim.orientation.angle_between(offset.turn) < 1e-5);
    }

    #[test]
    fn a_body_on_its_weighted_support_target_does_not_apply_the_weight_twice() {
        let weighted = pose(Vec3::new(0.3, 1.0, -0.5), Quat::from_rotation_x(-0.35));
        let offset = RenderOffset::between(weighted, weighted, 1.0, 0.0, 1.5);
        assert!(offset.is_none());
        let mut hand = CockpitHand {
            wrist: weighted,
            grip: weighted,
            aim: weighted,
        };
        offset.apply(&mut hand, Vec3::Z * -0.06);
        assert_eq!(hand.wrist.position, weighted.position);
        assert!(hand.wrist.orientation.angle_between(weighted.orientation) < 1e-4);
    }

    #[test]
    fn platform_translation_and_turn_do_not_change_the_local_collision_response() {
        let palm = pose(Vec3::new(0.3, 1.0, -0.4), Quat::from_rotation_x(0.2));
        let body = pose(
            palm.position + Vec3::Z * 0.1,
            Quat::from_rotation_y(0.3) * palm.orientation,
        );
        let expected = RenderOffset::between(palm, body, 1.0, 0.0, 1.5);
        for yaw in [0.0, 0.3, -0.8, 1.2] {
            let rotation = Quat::from_rotation_y(yaw);
            let translation = Vec3::new(50.0, 3.0, -20.0);
            let carried = |p: Pose| {
                pose(
                    translation + rotation * p.position,
                    rotation * p.orientation,
                )
            };
            let measured = RenderOffset::between(carried(palm), carried(body), 1.0, 0.0, 1.5)
                .to_cockpit(rotation, 1.0);
            assert!(measured.shift.abs_diff_eq(expected.shift, 1e-5));
            assert!(measured.turn.angle_between(expected.turn) < 1e-4);
        }
    }

    #[test]
    fn the_latest_step_is_visible_immediately_without_an_extra_blend_interval() {
        let now = Instant::now();
        let mut timeline = OffsetTimeline::EMPTY;
        assert!(timeline.at(now).is_none());
        timeline.push([RenderOffset::NONE; 2], now);
        let latest = [RenderOffset {
            shift: Vec3::X * 0.1,
            turn: Quat::IDENTITY,
        }; 2];
        timeline.push(latest, now + Duration::from_millis(20));
        assert_eq!(timeline.at(now + Duration::from_millis(20)), Some(latest));
        timeline.reset();
        assert!(timeline.at(now).is_none());
    }
}
