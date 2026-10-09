//! Where the player's feet are, estimated from the tracked head. Pure.
//!
//! Nothing tracks the feet, and the head is a poor stand-in for them: a player leaning over a
//! table or reaching for something on the floor moves the head well ahead of feet that stayed put.
//! The roomscale chase and the procedural body's steps under the head followed the head, so a
//! reach carried Rico's capsule and feet into what the hands reached for, and the hands' bodies
//! pushed the player back off it.
//!
//! A lean from the hips swings the neck pivot about the hip joints, so it lowers the pivot as it
//! moves it out: a pivot `L` above the hips that has dropped `d` below the player's standing
//! height leans out by up to `√(L² − (L − d)²)` ([`implied_lean`]). The feet ([`StanceTracker`])
//! stay put while the pivot is within that lean of them, plus a little slack for an upright head's
//! sway, and at most [`TorsoConfig::lean_reach_m`] (about an arm's length); a pivot further out
//! than that has stepped, and the feet follow it to within that distance. Upright, with no lean
//! implied, they settle back under the pivot, so a player who walks across the room has them
//! underneath again once they stop. The standing height is the one the procedural body measures
//! the crouch from ([`StandingReference`]): a high percentile of the pivot's height while the gaze
//! is level.
//!
//! Positions are in the cockpit frame: +X right, +Y up, -Z forward, metres; the horizontal ones
//! are `(x, z)`.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::{
    body::{
        BodyDimensions,
        procedural::head::{StandingReference, head_drop, looks_level},
        torso::{EYES_TO_NECK_PIVOT, TorsoConfig},
    },
    weapons::volumes::HIP_HEIGHT_FRACTION,
};

/// Where the feet are, and how far the neck pivot leans out over them.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StanceEstimate {
    /// The middle of the feet, `(x, z)`.
    pub stance: Vec2,
    /// The neck pivot's horizontal offset from over the feet, `(x, z)`: what a lean or a reach
    /// has moved the head without the feet.
    pub lean: Vec2,
    /// How far out the pivot's drop says it may lean, metres.
    pub implied_lean_m: f32,
}

/// Follows the middle of the player's feet from the neck pivot.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StanceTracker {
    stance: Option<Vec2>,
    standing: StandingReference,
}

impl StanceTracker {
    pub const fn new() -> Self {
        Self {
            stance: None,
            standing: StandingReference::new(),
        }
    }

    /// Start over: the next update puts the feet under the pivot.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Turn the held feet by `turn` about +Y, for a cockpit frame that turned by it (new frame from
    /// old).
    pub fn rotate(&mut self, turn: Quat) {
        if let Some(stance) = self.stance.as_mut() {
            let turned = turn * Vec3::new(stance.x, 0.0, stance.y);
            if turned.is_finite() {
                *stance = Vec2::new(turned.x, turned.z);
            }
        }
    }

    /// Advance by `dt` seconds with the neck pivot at `neck` and the head turned `orientation`.
    /// `None` for a pivot that is not finite.
    pub fn update(
        &mut self,
        neck: Vec3,
        orientation: Quat,
        dt: f32,
        dims: &BodyDimensions,
        config: &TorsoConfig,
    ) -> Option<StanceEstimate> {
        if !neck.is_finite() {
            return None;
        }
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.25)
        } else {
            0.0
        };
        // The pivot's height over where it is when the player stands at the recentre's height.
        let height = neck.y - EYES_TO_NECK_PIVOT.y;
        self.standing.observe(height, looks_level(orientation), dt);
        let drop = head_drop(height, self.standing.reference(), DROP_DEADBAND_M);
        let reach = config.lean_reach_m.max(0.0);
        let slack = config.upright_slack_m.clamp(0.0, reach.max(0.0));
        let implied = implied_lean(drop, pivot_over_hips(dims)).min(reach);
        let allowed = (slack + implied).min(reach.max(slack));

        let pivot = Vec2::new(neck.x, neck.z);
        let mut stance = self.stance.unwrap_or(pivot);
        let out = pivot - stance;
        let distance = out.length();
        if distance > allowed && distance.is_finite() {
            stance = pivot - out / distance * allowed;
        }
        // Upright, the feet settle back under the pivot; leaning, they stay.
        let upright = if slack > 0.0 {
            1.0 - (implied / slack).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let settle = (1.0 - (-dt / SETTLE_S).exp()) * upright;
        stance += (pivot - stance) * settle;
        if !stance.is_finite() {
            stance = pivot;
        }
        self.stance = Some(stance);
        Some(StanceEstimate {
            stance,
            lean: pivot - stance,
            implied_lean_m: implied,
        })
    }
}

/// How far out a neck pivot `over_hips_m` above the hip joints leans when it has dropped `drop_m`,
/// swung about them: `√(L² − (L − d)²)`, the full length once it has dropped that far.
pub fn implied_lean(drop_m: f32, over_hips_m: f32) -> f32 {
    let length = if over_hips_m.is_finite() {
        over_hips_m.max(0.0)
    } else {
        0.0
    };
    if !drop_m.is_finite() || drop_m <= 0.0 {
        return 0.0;
    }
    if drop_m >= length {
        return length;
    }
    let below = length - drop_m;
    (length * length - below * below).max(0.0).sqrt()
}

/// How far the neck pivot sits above the hip joints of a player standing upright, metres.
fn pivot_over_hips(dims: &BodyDimensions) -> f32 {
    (dims.eye_height + EYES_TO_NECK_PIVOT.y - dims.height * HIP_HEIGHT_FRACTION)
        .max(MIN_OVER_HIPS_M)
}

/// The pivot's drop below the standing height that is a standing head's bob rather than a lean,
/// metres.
const DROP_DEADBAND_M: f32 = 0.02;

/// How quickly the feet settle under an upright pivot, seconds (a time constant).
const SETTLE_S: f32 = 0.5;

/// The least the pivot is taken to sit above the hips, metres, for body dimensions that would put
/// it lower.
const MIN_OVER_HIPS_M: f32 = 0.3;

#[cfg(test)]
mod tests {
    use super::*;

    const DIMS: BodyDimensions = BodyDimensions::GENERIC_ADULT;
    const DT: f32 = 1.0 / 90.0;

    /// The neck pivot of a player standing at the recentre's height, `(x, z)` = `at`, leaning
    /// from the hips by `lean_deg` toward -Z.
    fn neck(at: Vec2, lean_deg: f32) -> Vec3 {
        let length = pivot_over_hips(&DIMS);
        let angle = lean_deg.to_radians();
        let standing = EYES_TO_NECK_PIVOT.y;
        Vec3::new(
            at.x,
            standing - length * (1.0 - angle.cos()),
            at.y - length * angle.sin(),
        )
    }

    fn run(tracker: &mut StanceTracker, neck: Vec3, seconds: f32) -> StanceEstimate {
        let config = TorsoConfig::new();
        let mut last = None;
        for _ in 0..(seconds / DT).round() as usize {
            last = tracker.update(neck, Quat::IDENTITY, DT, &DIMS, &config);
        }
        last.unwrap()
    }

    #[test]
    fn the_lean_follows_the_pivot_round_the_hips() {
        assert_eq!(implied_lean(0.0, 0.7), 0.0);
        assert_eq!(implied_lean(-0.1, 0.7), 0.0);
        assert!(
            (implied_lean(0.7 - 0.7 * 45f32.to_radians().cos(), 0.7) - 0.7 * 0.5f32.sqrt()).abs()
                < 1e-5
        );
        assert_eq!(implied_lean(2.0, 0.7), 0.7);
        assert_eq!(implied_lean(f32::NAN, 0.7), 0.0);
    }

    #[test]
    fn leaning_over_a_table_leaves_the_feet() {
        let mut tracker = StanceTracker::new();
        let start = run(&mut tracker, neck(Vec2::ZERO, 0.0), 3.0);
        assert!(start.stance.length() < 1e-4);
        // Lean forty degrees from the hips and hold it: the head goes about half a metre out.
        let leaning = run(&mut tracker, neck(Vec2::ZERO, 40.0), 5.0);
        assert!(leaning.lean.length() > 0.35, "{leaning:?}");
        assert!(leaning.stance.length() < 0.06, "{leaning:?}");
        // Upright again, the feet are still under the head.
        let back = run(&mut tracker, neck(Vec2::ZERO, 0.0), 2.0);
        assert!(back.stance.length() < 0.06, "{back:?}");
    }

    #[test]
    fn walking_upright_carries_the_feet() {
        let mut tracker = StanceTracker::new();
        run(&mut tracker, neck(Vec2::ZERO, 0.0), 3.0);
        // Walk a metre and a half to the right at a metre a second, upright.
        let config = TorsoConfig::new();
        let mut last = None;
        for i in 1..=(1.5 / DT).round() as usize {
            let at = Vec2::new(i as f32 * DT, 0.0);
            last = tracker.update(neck(at, 0.0), Quat::IDENTITY, DT, &DIMS, &config);
        }
        let walking = last.unwrap();
        assert!(
            walking.lean.length() <= config.upright_slack_m + 1e-4,
            "{walking:?}"
        );
        // Stopped, the feet settle under the pivot.
        let stopped = run(&mut tracker, neck(Vec2::new(1.5, 0.0), 0.0), 2.0);
        assert!(stopped.lean.length() < 0.01, "{stopped:?}");
    }

    #[test]
    fn a_pivot_past_an_arms_length_has_stepped() {
        let mut tracker = StanceTracker::new();
        run(&mut tracker, neck(Vec2::ZERO, 0.0), 3.0);
        let config = TorsoConfig::new();
        // Crouched low and a metre out: further than any lean, so the feet follow to within reach.
        let far = Vec3::new(0.0, EYES_TO_NECK_PIVOT.y - 0.6, -1.0);
        let estimate = run(&mut tracker, far, 0.5);
        assert!(
            (estimate.lean.length() - config.lean_reach_m).abs() < 1e-3,
            "{estimate:?}"
        );
    }

    #[test]
    fn a_turned_frame_turns_the_feet() {
        let mut tracker = StanceTracker::new();
        run(&mut tracker, neck(Vec2::new(0.0, -0.5), 0.0), 3.0);
        tracker.rotate(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let config = TorsoConfig::new();
        let turned = tracker
            .update(
                Vec3::new(-0.5, EYES_TO_NECK_PIVOT.y, 0.0),
                Quat::IDENTITY,
                0.0,
                &DIMS,
                &config,
            )
            .unwrap();
        assert!(turned.lean.length() < 1e-3, "{turned:?}");
        assert_eq!(
            tracker.update(Vec3::NAN, Quat::IDENTITY, DT, &DIMS, &config),
            None
        );
    }
}
