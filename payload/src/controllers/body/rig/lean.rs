//! The lean the chase leaves alone. Pure.
//!
//! A player leaning over a table or reaching to the floor moves the head out over feet that stay
//! put. A lean from the hips lowers the neck pivot as it moves it out, so how far the pivot has
//! dropped below the player's standing height bounds how far out it may be without a step:
//! [`allowance`], linear past a deadband for a standing head's bob, and capped. A crouch drops the
//! pivot straight down, so it leaves nothing to absorb; a lean drops it and moves it forward, and
//! the allowance absorbs the forward part.
//!
//! The allowance is widest along the torso's forward and narrower to the side and behind
//! ([`within`]): the part of the gap inside that ellipse is the lean, and the chase closes what lies
//! outside it. Nothing is held between frames but the standing height ([`StandingHeight`]), so
//! standing back up removes the lean and the gap together, with no root motion and nothing to
//! forget.
//!
//! Positions are in the cockpit frame: +X right, +Y up, -Z forward, metres; the horizontal ones
//! are `(x, z)`.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::{
    procedural::head::{StandingReference, head_drop},
    rig::config::LeanConfig,
    torso::{EYES_TO_NECK_PIVOT, yaw_forward},
};

/// One frame's lean.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Lean {
    /// How far the neck pivot is below the player's standing height, metres.
    pub drop_m: f32,
    /// How far forward the pivot may be out over the feet, metres.
    pub allowance_m: f32,
    /// The part of the gap the lean accounts for, `(x, z)`.
    pub vector: Vec2,
}

/// The lean for a horizontal `gap` from the feet to the ground point under the upright eyes, a
/// torso facing `yaw`, and the pivot `drop_m` below the standing height.
pub fn lean(gap: Vec2, yaw: f32, drop_m: f32, config: &LeanConfig) -> Lean {
    let allowance_m = allowance(drop_m, config);
    Lean {
        drop_m,
        allowance_m,
        vector: within(gap, yaw, allowance_m, config),
    }
}

/// How far forward the neck pivot may be out over the feet when it has dropped `drop_m` below the
/// standing height: `gain · (drop − deadband)`, between zero and the cap. Zero with the lean off.
pub fn allowance(drop_m: f32, config: &LeanConfig) -> f32 {
    if !config.enabled {
        return 0.0;
    }
    let allowance = config.gain * (drop_m - config.deadband_m);
    if allowance.is_finite() {
        allowance.clamp(0.0, config.max_m.max(0.0))
    } else {
        0.0
    }
}

/// The part of `gap` inside the lean ellipse of a torso facing `yaw`: `allowance` forward, its
/// side share to either side, and its back share behind. A gap inside it is all lean; a gap
/// outside it is scaled back along itself onto the ellipse.
pub fn within(gap: Vec2, yaw: f32, allowance: f32, config: &LeanConfig) -> Vec2 {
    if !gap.is_finite() || !allowance.is_finite() || allowance <= 0.0 {
        return Vec2::ZERO;
    }
    let forward = flat(yaw_forward(yaw));
    let right = flat(Quat::from_rotation_y(yaw) * Vec3::X);
    let along = gap.dot(forward);
    let across = gap.dot(right);
    let share = if along >= 0.0 { 1.0 } else { config.back_share };
    let forward_axis = (allowance * share.clamp(0.0, 1.0)).max(MIN_AXIS_M);
    let side_axis = (allowance * config.side_share.clamp(0.0, 1.0)).max(MIN_AXIS_M);
    let extent = (along / forward_axis).powi(2) + (across / side_axis).powi(2);
    if extent <= 1.0 {
        gap
    } else {
        gap / extent.sqrt()
    }
}

/// The neck pivot's height over where it is for a player standing at the cockpit's origin
/// height, for eyes at `eyes` turned `head`: the pivot is below and behind the eyes in the head's
/// frame, so a nod moves the eyes and not the pivot.
pub fn pivot_height(eyes: Vec3, head: Quat) -> f32 {
    (eyes + head.normalize() * EYES_TO_NECK_PIVOT).y - EYES_TO_NECK_PIVOT.y
}

/// The height the player is seen to stand at, and how far the pivot is below it.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StandingHeight {
    reference: StandingReference,
    /// The runtime's floor jumps last seen; a new one re-measures the standing height.
    floor_jumps: u32,
}

impl StandingHeight {
    pub const fn new() -> Self {
        Self {
            reference: StandingReference::new(),
            floor_jumps: 0,
        }
    }

    /// Advance by `dt` seconds with the pivot at `height`, taking it as a standing sample when
    /// `standing` (on foot, the gaze about level), and return how far the pivot is below the
    /// standing height, metres. `floor_jumps` is the runtime's count of floor jumps.
    pub fn observe(&mut self, height: f32, standing: bool, dt: f32, floor_jumps: u32) -> f32 {
        if floor_jumps != self.floor_jumps {
            self.floor_jumps = floor_jumps;
            self.reference.reseed();
        }
        self.reference.observe(height, standing, dt);
        head_drop(height, self.reference.reference(), 0.0)
    }

    /// The standing height, metres over the cockpit's origin height.
    pub fn reference(&self) -> f32 {
        self.reference.reference()
    }
}

/// The smallest semi-axis the ellipse is given, metres, so that a share of zero clamps that
/// direction to nothing rather than dividing by zero.
const MIN_AXIS_M: f32 = 1e-6;

fn flat(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: LeanConfig = LeanConfig::new();
    const FORWARD: Vec2 = Vec2::new(0.0, -1.0);
    const RIGHT: Vec2 = Vec2::new(1.0, 0.0);

    #[test]
    fn the_allowance_is_linear_past_the_deadband_and_capped() {
        assert_eq!(allowance(0.0, &CONFIG), 0.0);
        assert_eq!(allowance(0.04, &CONFIG), 0.0);
        assert!((allowance(0.1, &CONFIG) - 0.24).abs() < 1e-6);
        assert_eq!(allowance(1.0, &CONFIG), 0.35);
        assert_eq!(allowance(f32::NAN, &CONFIG), 0.0);
        let off = LeanConfig {
            enabled: false,
            ..CONFIG
        };
        assert_eq!(allowance(0.3, &off), 0.0);
    }

    #[test]
    fn the_ellipse_is_widest_forward() {
        let a = 0.2;
        // Inside: all of it is lean.
        assert_eq!(within(FORWARD * 0.15, 0.0, a, &CONFIG), FORWARD * 0.15);
        // Outside forward, sideways, and backward: clamped to each semi-axis.
        assert!((within(FORWARD * 0.5, 0.0, a, &CONFIG) - FORWARD * 0.2).length() < 1e-6);
        assert!((within(RIGHT * 0.5, 0.0, a, &CONFIG) - RIGHT * 0.1).length() < 1e-6);
        assert!((within(-FORWARD * 0.5, 0.0, a, &CONFIG) - -FORWARD * 0.05).length() < 1e-6);
        // A torso turned a quarter left faces -X.
        let left = std::f32::consts::FRAC_PI_2;
        assert!((within(-RIGHT * 0.5, left, a, &CONFIG) - -RIGHT * 0.2).length() < 1e-5);
        assert_eq!(within(FORWARD, 0.0, 0.0, &CONFIG), Vec2::ZERO);
        assert_eq!(within(Vec2::NAN, 0.0, a, &CONFIG), Vec2::ZERO);
    }

    #[test]
    fn a_crouch_leaves_nothing_and_a_lean_absorbs_its_forward_part() {
        // Straight down: the gap is nothing, so neither is the lean.
        let crouch = lean(Vec2::ZERO, 0.0, 0.4, &CONFIG);
        assert_eq!(crouch.vector, Vec2::ZERO);
        assert_eq!(crouch.allowance_m, 0.35);
        // Thirty centimetres forward with a twelve-centimetre drop: all of it is lean.
        let leaning = lean(FORWARD * 0.3, 0.0, 0.12, &CONFIG);
        assert!((leaning.vector - FORWARD * 0.3).length() < 1e-5);
        // With an eight-centimetre drop, sixteen centimetres of it is; the chase closes the rest.
        let shallow = lean(FORWARD * 0.3, 0.0, 0.08, &CONFIG);
        assert!((shallow.vector - FORWARD * 0.16).length() < 1e-5);
        // Standing up removes the allowance, and with it the lean.
        let upright = lean(FORWARD * 0.3, 0.0, 0.0, &CONFIG);
        assert_eq!(upright.vector, Vec2::ZERO);
    }

    #[test]
    fn a_nod_does_not_move_the_pivot() {
        assert!(pivot_height(Vec3::ZERO, Quat::IDENTITY).abs() < 1e-6);
        let down = Quat::from_rotation_x(-60f32.to_radians());
        let eyes = down * -EYES_TO_NECK_PIVOT + EYES_TO_NECK_PIVOT;
        assert!(pivot_height(eyes, down).abs() < 1e-5);
        assert!((pivot_height(Vec3::Y * -0.3, Quat::IDENTITY) + 0.3).abs() < 1e-6);
    }

    #[test]
    fn a_floor_jump_remeasures_the_standing_height() {
        let mut standing = StandingHeight::new();
        for _ in 0..400 {
            standing.observe(-0.08, true, 0.05, 0);
        }
        assert!((standing.reference() + 0.08).abs() < 1e-6);
        assert!(standing.observe(-0.08, true, 0.05, 0).abs() < 1e-6);
        assert!((standing.observe(-0.08, true, 0.05, 1) - 0.08).abs() < 1e-6);
        assert_eq!(standing.reference(), 0.0);
    }
}
