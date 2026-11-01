//! The lean the chase leaves alone. Pure.
//!
//! A player leaning over a table or reaching to the floor moves the head out over feet that stay
//! put. A lean from the hips lowers the neck pivot as it moves it out, so how far the pivot has
//! dropped below the player's standing height bounds how far out it may be without a step:
//! [`allowance`], linear past a deadband for a head's bob, and capped. A crouch drops the pivot
//! straight down, so it leaves nothing to absorb; a lean drops it and moves it forward, and the
//! allowance absorbs the forward part.
//!
//! The allowance's shape is the posture's ([`LeanShape`]). A seated player leans from the hips over
//! the chair and cannot step, so a seated lean starts at a smaller drop and reaches further to the
//! side ([`LeanShape::SEATED`]). The seated drop is measured from the seated eye reference
//! ([`seated_drop`]) rather than from a standing height.
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

use crate::controllers::{
    body::{
        procedural::head::{StandingReference, head_drop},
        rig::{
            config::LeanConfig,
            torso::{EYES_TO_NECK_PIVOT, yaw_forward},
        },
    },
    roomscale::posture::EffectivePosture,
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

/// How the lean allowance grows with the pivot's drop, and its shape around the torso, for one
/// posture.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LeanShape {
    /// Metres of lean allowed per metre of drop past the deadband.
    pub gain: f32,
    /// The drop that allows no lean, metres: the head's bob.
    pub deadband_m: f32,
    /// The furthest lean allowed forward of the torso, metres.
    pub max_m: f32,
    /// The lean allowed to either side, as a share of the forward lean.
    pub side_share: f32,
    /// The lean allowed backward, as a share of the forward lean.
    pub back_share: f32,
}

impl LeanShape {
    /// A standing player's: a standing head bobs by a few centimetres, and a player who reaches
    /// further to the side than a lean takes steps.
    pub const STANDING: Self = Self {
        gain: 4.0,
        deadband_m: 0.04,
        max_m: 0.35,
        side_share: 0.5,
        back_share: 0.25,
    };

    /// A seated player's. A seated head at rest drops by millimetres, and a seated lean from the
    /// hips drops the pivot little for the reach it makes, so the deadband is a centimetre. A
    /// seated player cannot step to reach to the side and leans instead, so the side share is
    /// wider. A seated head that moves back has shifted in the chair against the backrest rather
    /// than leaned, so the back share is the standing one and the chase follows it.
    pub const SEATED: Self = Self {
        gain: 4.0,
        deadband_m: 0.01,
        max_m: 0.35,
        side_share: 0.75,
        back_share: 0.25,
    };

    /// The shape for a posture.
    pub fn for_posture(posture: EffectivePosture) -> Self {
        match posture {
            EffectivePosture::Seated => Self::SEATED,
            EffectivePosture::Standing => Self::STANDING,
        }
    }
}

/// The lean for a horizontal `gap` from the feet to the ground point under the upright eyes, a
/// torso facing `yaw`, and the pivot `drop_m` below the upright height, in a posture's `shape`.
pub fn lean(gap: Vec2, yaw: f32, drop_m: f32, shape: &LeanShape, config: &LeanConfig) -> Lean {
    let allowance_m = allowance(drop_m, shape, config);
    Lean {
        drop_m,
        allowance_m,
        vector: within(gap, yaw, allowance_m, shape),
    }
}

/// How far forward the neck pivot may be out over the feet when it has dropped `drop_m` below the
/// upright height: the shape's gain `· (drop −` deadband`)`, between zero and its cap. Zero with
/// the lean off.
pub fn allowance(drop_m: f32, shape: &LeanShape, config: &LeanConfig) -> f32 {
    if !config.enabled {
        return 0.0;
    }
    let allowance = shape.gain * (drop_m - shape.deadband_m);
    if allowance.is_finite() {
        allowance.clamp(0.0, shape.max_m)
    } else {
        0.0
    }
}

/// The part of `gap` inside the lean ellipse of a torso facing `yaw`: `allowance` forward, the
/// shape's side share of it to either side, and its back share behind. A gap inside it is all
/// lean; a gap outside it is scaled back along itself onto the ellipse.
pub fn within(gap: Vec2, yaw: f32, allowance: f32, shape: &LeanShape) -> Vec2 {
    if !gap.is_finite() || !allowance.is_finite() || allowance <= 0.0 {
        return Vec2::ZERO;
    }
    let forward = flat(yaw_forward(yaw));
    let right = flat(Quat::from_rotation_y(yaw) * Vec3::X);
    let along = gap.dot(forward);
    let across = gap.dot(right);
    let share = if along >= 0.0 { 1.0 } else { shape.back_share };
    let forward_axis = (allowance * share).max(MIN_AXIS_M);
    let side_axis = (allowance * shape.side_share).max(MIN_AXIS_M);
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

/// How far a seated player's neck pivot at `height` is below where it is while they sit at the
/// seated eye reference, metres. The seated head height puts the reference at the cockpit's origin
/// height, so the upright seated pivot is at zero; the reference adapts to how the player sits, so
/// nothing is sampled here.
pub fn seated_drop(height: f32) -> f32 {
    head_drop(height, 0.0, 0.0)
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
    const STANDING: LeanShape = LeanShape::STANDING;
    const SEATED: LeanShape = LeanShape::SEATED;
    const FORWARD: Vec2 = Vec2::new(0.0, -1.0);
    const RIGHT: Vec2 = Vec2::new(1.0, 0.0);

    #[test]
    fn the_allowance_is_linear_past_the_deadband_and_capped() {
        assert_eq!(allowance(0.0, &STANDING, &CONFIG), 0.0);
        assert_eq!(allowance(0.04, &STANDING, &CONFIG), 0.0);
        assert!((allowance(0.1, &STANDING, &CONFIG) - 0.24).abs() < 1e-6);
        assert_eq!(allowance(1.0, &STANDING, &CONFIG), 0.35);
        assert_eq!(allowance(f32::NAN, &STANDING, &CONFIG), 0.0);
        let off = LeanConfig { enabled: false };
        assert_eq!(allowance(0.3, &STANDING, &off), 0.0);
    }

    #[test]
    fn the_ellipse_is_widest_forward() {
        let a = 0.2;
        // Inside: all of it is lean.
        assert_eq!(within(FORWARD * 0.15, 0.0, a, &STANDING), FORWARD * 0.15);
        // Outside forward, sideways, and backward: clamped to each semi-axis.
        assert!((within(FORWARD * 0.5, 0.0, a, &STANDING) - FORWARD * 0.2).length() < 1e-6);
        assert!((within(RIGHT * 0.5, 0.0, a, &STANDING) - RIGHT * 0.1).length() < 1e-6);
        assert!((within(-FORWARD * 0.5, 0.0, a, &STANDING) - -FORWARD * 0.05).length() < 1e-6);
        // A torso turned a quarter left faces -X.
        let left = std::f32::consts::FRAC_PI_2;
        assert!((within(-RIGHT * 0.5, left, a, &STANDING) - -RIGHT * 0.2).length() < 1e-5);
        assert_eq!(within(FORWARD, 0.0, 0.0, &STANDING), Vec2::ZERO);
        assert_eq!(within(Vec2::NAN, 0.0, a, &STANDING), Vec2::ZERO);
    }

    #[test]
    fn a_crouch_leaves_nothing_and_a_lean_absorbs_its_forward_part() {
        // Straight down: the gap is nothing, so neither is the lean.
        let crouch = lean(Vec2::ZERO, 0.0, 0.4, &STANDING, &CONFIG);
        assert_eq!(crouch.vector, Vec2::ZERO);
        assert_eq!(crouch.allowance_m, 0.35);
        // Thirty centimetres forward with a twelve-centimetre drop: all of it is lean.
        let leaning = lean(FORWARD * 0.3, 0.0, 0.12, &STANDING, &CONFIG);
        assert!((leaning.vector - FORWARD * 0.3).length() < 1e-5);
        // With an eight-centimetre drop, sixteen centimetres of it is; the chase closes the rest.
        let shallow = lean(FORWARD * 0.3, 0.0, 0.08, &STANDING, &CONFIG);
        assert!((shallow.vector - FORWARD * 0.16).length() < 1e-5);
        // Standing up removes the allowance, and with it the lean.
        let upright = lean(FORWARD * 0.3, 0.0, 0.0, &STANDING, &CONFIG);
        assert_eq!(upright.vector, Vec2::ZERO);
    }

    #[test]
    fn a_seated_lean_counts_from_a_smaller_drop() {
        // The seated session's leans: a head 15 cm forward of the chair has dropped about 5 cm,
        // and one 22 cm forward about 9 cm.
        let reach = lean(FORWARD * 0.15, 0.0, 0.05, &SEATED, &CONFIG);
        assert!((reach.vector - FORWARD * 0.15).length() < 1e-5, "{reach:?}");
        let far = lean(FORWARD * 0.22, 0.0, 0.09, &SEATED, &CONFIG);
        assert!((far.vector - FORWARD * 0.22).length() < 1e-5, "{far:?}");
        // The standing shape leaves most of the first to the chase.
        let standing = lean(FORWARD * 0.15, 0.0, 0.05, &STANDING, &CONFIG);
        assert!(
            (standing.vector - FORWARD * 0.04).length() < 1e-5,
            "{standing:?}"
        );
        // A seated head at rest bobs by millimetres, which allow nothing.
        assert_eq!(allowance(0.008, &SEATED, &CONFIG), 0.0);
        assert_eq!(allowance(1.0, &SEATED, &CONFIG), 0.35);
    }

    #[test]
    fn a_seated_lean_reaches_further_to_the_side_but_not_back() {
        let a = allowance(0.09, &SEATED, &CONFIG);
        assert!((a - 0.32).abs() < 1e-5);
        let side = within(RIGHT * 0.5, 0.0, a, &SEATED);
        assert!((side - RIGHT * 0.24).length() < 1e-5, "{side:?}");
        assert!((within(RIGHT * 0.5, 0.0, a, &STANDING) - RIGHT * 0.16).length() < 1e-5);
        // Back against the chair is a shift the chase follows, as standing.
        let back = within(-FORWARD * 0.5, 0.0, a, &SEATED);
        assert!((back - -FORWARD * 0.08).length() < 1e-5, "{back:?}");
    }

    #[test]
    fn a_seated_shift_with_no_drop_is_all_chase() {
        // Sliding back in the chair, the head level: nothing of it is lean.
        let shift = lean(-FORWARD * 0.4, 0.0, seated_drop(0.0), &SEATED, &CONFIG);
        assert_eq!(shift.allowance_m, 0.0);
        assert_eq!(shift.vector, Vec2::ZERO);
    }

    #[test]
    fn the_seated_drop_is_measured_from_the_seated_reference() {
        assert_eq!(seated_drop(0.0), 0.0);
        assert_eq!(seated_drop(0.05), 0.0);
        assert!((seated_drop(-0.07) - 0.07).abs() < 1e-6);
        assert_eq!(seated_drop(f32::NAN), 0.0);
    }

    #[test]
    fn each_posture_has_its_shape() {
        assert_eq!(LeanShape::for_posture(EffectivePosture::Seated), SEATED);
        assert_eq!(LeanShape::for_posture(EffectivePosture::Standing), STANDING);
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
