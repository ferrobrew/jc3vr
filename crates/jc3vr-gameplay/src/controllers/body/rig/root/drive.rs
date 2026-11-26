//! The step drive's arithmetic between the frame half and the simulation ticks, and the warp test.
//! Pure.
//!
//! The frame half advances the grounded rig once per rendered frame
//! ([`crate::controllers::body::rig::grounded`]) and publishes the step phase with the gap from
//! Rico's pelvis to the floor point under the player's neck, carried into the world, and the
//! completed movement the cockpit shift had folded in when it was measured ([`StepFrame`]). Each
//! simulation tick takes away what the drive has completed since ([`StepFrame::effective_gap`]),
//! so two ticks in one frame never close the same gap twice.
//!
//! Cockpit vectors are `(x, z)` in the cockpit frame (+X right, -Z forward); world vectors are
//! world `(x, z)`.

use body_rig::StepPhase;
use glam::{Quat, Vec2, Vec3};

/// What the frame half publishes for the simulation half.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StepFrame {
    pub phase: StepPhase,
    /// The pelvis's gap to the floor point under the neck, world `(x, z)`, metres.
    pub gap: Vec2,
    /// The cumulative completed movement the cockpit shift had folded in when the gap was
    /// measured, world `(x, z)`.
    pub observed: Vec2,
}

impl StepFrame {
    /// The gap left once the movement completed since the frame is taken away, for the cumulative
    /// completed movement `completed`.
    pub fn effective_gap(&self, completed: Vec2) -> Vec2 {
        let gap = self.gap - (completed - self.observed);
        if gap.is_finite() { gap } else { Vec2::ZERO }
    }
}

/// A cockpit `(x, z)` vector carried into the world by the body rotation `view` and the world
/// scale, world `(x, z)`.
pub fn to_world(v: Vec2, view: Quat, world_scale: f32) -> Vec2 {
    let world = view.normalize() * Vec3::new(v.x, 0.0, v.y) * world_scale;
    let flat = Vec2::new(world.x, world.z);
    if flat.is_finite() { flat } else { Vec2::ZERO }
}

/// A world `(x, z)` vector carried into the cockpit frame by the inverse of [`to_world`].
pub fn to_cockpit(v: Vec2, view: Quat, world_scale: f32) -> Vec3 {
    let cockpit = view.normalize().inverse() * Vec3::new(v.x, 0.0, v.y) / world_scale;
    let flat = Vec3::new(cockpit.x, 0.0, cockpit.z);
    if flat.is_finite() { flat } else { Vec3::ZERO }
}

/// The shortest single-tick jump of the body, metres, that counts as a teleport the game made
/// without announcing it. The threshold also grows with the body's speed ([`is_warp`]).
pub const WARP_MIN_M: f32 = 8.0;

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

    const DT: f32 = 1.0 / 30.0;

    #[test]
    fn completed_movement_since_the_frame_comes_off_the_gap() {
        let frame = StepFrame {
            phase: StepPhase::Stepping,
            gap: Vec2::new(0.3, 0.0),
            observed: Vec2::new(1.0, 0.0),
        };
        let completed = Vec2::new(1.12, 0.0);
        assert!((frame.effective_gap(completed) - Vec2::new(0.18, 0.0)).length() < 1e-6);
    }

    #[test]
    fn the_cockpit_carries_into_the_world_by_the_view_and_the_scale() {
        let quarter = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let world = to_world(Vec2::new(0.0, -1.0), quarter, 2.0);
        assert!((world - Vec2::new(-2.0, 0.0)).length() < 1e-5, "{world}");
        let back = to_cockpit(world, quarter, 2.0);
        assert!((back - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-5, "{back}");
    }

    #[test]
    fn a_warp_is_far_and_faster_than_the_body_moves() {
        assert!(!is_warp(0.5, 5.0, DT, 8.0));
        assert!(is_warp(20.0, 5.0, DT, 8.0));
        // A diving wingsuit covers ten metres a tick at 300 m/s.
        assert!(!is_warp(10.0, 300.0, DT, 8.0));
        assert!(!is_warp(f32::NAN, 0.0, 0.0, 8.0));
    }
}
