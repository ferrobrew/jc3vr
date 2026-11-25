//! The pelvis solver: whether the player's head is out over Rico's pelvis because the player leans,
//! or because the player stepped.
//!
//! The spine is one segment of the calibrated length from the waist pivot, over the pelvis at the
//! calibrated hip height, to the neck pivot under the eyes. A bend is the segment's tilt from
//! vertical, within limits that are widest forward, narrower to the side, and narrowest back. A
//! bend that carries the neck out horizontally also lowers it, along the segment's arc, so the
//! neck's height bounds how far out a bend can explain it:
//!
//! - a head turning or nodding moves the eyes about the neck pivot and moves nothing below it;
//! - a bend from the waist moves the neck out and down along the arc: a lean, and the pelvis stays;
//! - a step moves the neck out without the drop the arc demands: the excess over what a legal bend
//!   at the neck's height explains is step demand;
//! - a drop of the head that no bend explains is a crouch.
//!
//! Near vertical the arc is flat, so a few centimetres of drop explain a large reach, and a
//! standing head's bob would read as a lean or a step at random. The lean radius sets a floor under
//! the reach a bend explains: inside it Rico never moves, and the spine absorbs the offset. The
//! hands are a tie-breaker: both reaching out along the offset widen the reach, so a slow drift
//! toward something held in both hands stays a lean longer. Behind the pelvis the floor is a
//! smaller radius: a person stepping back steps rather than leaning back.
//!
//! A forward lean is a reach the player means, but an offset to the side or back held still is a
//! weight shift, which the step commit's idle drift walks the body under; the solve gives that
//! part of the offset as the drift.

use glam::{Quat, Vec2, Vec3};

use crate::{
    angle::{forward, right},
    body::neck_pivot,
};

/// The pelvis solver's tuning.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PelvisTuning {
    /// How far the neck may be out over the pelvis before a step is considered at all, metres:
    /// the floor under the reach a bend explains.
    pub lean_radius_m: f32,
    /// The floor under the reach a bend explains straight back, metres: the lean radius's
    /// counterpart behind the pelvis, between them toward the back quarters.
    pub back_radius_m: f32,
    /// The furthest the neck is ever out over the pelvis as a lean, metres, however low it is.
    pub max_lean_m: f32,
    /// The furthest the spine bends forward from vertical, degrees.
    pub forward_bend_deg: f32,
    /// The furthest it bends to either side, degrees.
    pub side_bend_deg: f32,
    /// The furthest it bends back, degrees.
    pub back_bend_deg: f32,
    /// How far ahead of the neck, along the offset, a hand reaches before it counts toward the
    /// tie-breaker, metres.
    pub hand_reach_m: f32,
    /// How much both hands reaching along the offset widen the reach a lean explains, metres.
    pub hand_lean_m: f32,
}

impl PelvisTuning {
    pub const fn new() -> Self {
        Self {
            lean_radius_m: 0.2,
            back_radius_m: 0.12,
            max_lean_m: 0.6,
            forward_bend_deg: 70.0,
            side_bend_deg: 30.0,
            back_bend_deg: 10.0,
            hand_reach_m: 0.3,
            hand_lean_m: 0.08,
        }
    }
}

impl Default for PelvisTuning {
    fn default() -> Self {
        Self::new()
    }
}

/// One solve's inputs, in the room frame: metres over the floor, +Y up, -Z forward.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PelvisInput {
    /// The eyes.
    pub eyes: Vec3,
    /// The head's orientation.
    pub head: Quat,
    /// The floor point under Rico's pelvis, `(x, z)`.
    pub pelvis: Vec2,
    /// The body's heading, which the bend's pitch and roll are measured against.
    pub body_yaw: f32,
    /// The tracked hands, left then right.
    pub hands: [Option<Vec3>; 2],
    /// The neck pivot's height while the player stands upright.
    pub standing_neck_m: f32,
    /// The waist pivot's standing height.
    pub hip_height_m: f32,
}

/// A bend of the spine from vertical, radians: pitch forward positive, roll toward the body's
/// right positive.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Bend {
    pub pitch: f32,
    pub roll: f32,
}

impl Bend {
    /// The whole tilt from vertical, radians.
    pub fn angle(&self) -> f32 {
        Vec2::new(self.pitch, self.roll).length()
    }
}

/// One solve's result, in the room frame.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PelvisSolve {
    /// The neck pivot.
    pub neck: Vec3,
    /// The waist pivot: over the pelvis, at the height the bend leaves it, never above the
    /// calibrated hip height.
    pub waist: Vec3,
    /// The spine's length, metres.
    pub spine_m: f32,
    /// The neck's horizontal offset from the pelvis, `(x, z)`.
    pub offset: Vec2,
    /// The bend that carries the neck out over the pelvis, within the limits.
    pub bend: Bend,
    /// How far out the neck may be as a lean at its height, metres.
    pub lean_reach_m: f32,
    /// The part of the offset past the lean's reach, `(x, z)`: what a step must take up.
    pub step_demand: Vec2,
    /// The part of the offset to the side of the body and behind it, `(x, z)`: what the idle drift
    /// walks the body under.
    pub drift: Vec2,
    /// How far the waist has dropped below the calibrated hip height: the head's drop that no bend
    /// explains, metres.
    pub crouch_m: f32,
    /// Whether both hands reach out along the offset.
    pub hands_reaching: bool,
}

impl PelvisSolve {
    /// The step demand's length, metres.
    pub fn demand_m(&self) -> f32 {
        self.step_demand.length()
    }
}

/// Solve one frame.
pub fn solve(input: &PelvisInput, tuning: &PelvisTuning) -> PelvisSolve {
    let neck = neck_pivot(input.eyes, input.head);
    if !neck.is_finite() || !input.pelvis.is_finite() {
        return PelvisSolve::default();
    }
    let spine = (input.standing_neck_m - input.hip_height_m).max(crate::body::MIN_SPINE_M);
    let spine = if spine.is_finite() {
        spine
    } else {
        crate::body::MIN_SPINE_M
    };
    let offset = Vec2::new(neck.x, neck.z) - input.pelvis;
    let reach = offset.length();
    let body_forward = forward(input.body_yaw);
    let body_right = right(input.body_yaw);
    let direction = offset.try_normalize().unwrap_or(body_forward);
    let (along, across) = (direction.dot(body_forward), direction.dot(body_right));
    let limit = bend_limit(along, across, tuning);

    // The tilt a bend needs to bring the neck down to its height from the standing neck's, about
    // the waist at the calibrated hip height.
    let rise = (neck.y - input.hip_height_m) / spine;
    let height_tilt = if rise >= 1.0 {
        0.0
    } else if rise <= -1.0 {
        std::f32::consts::PI
    } else {
        rise.acos()
    };
    let arc = spine * height_tilt.min(limit).sin();

    let hands_reaching = input.hands.iter().all(|hand| {
        hand.is_some_and(|hand| {
            (Vec2::new(hand.x, hand.z) - Vec2::new(neck.x, neck.z)).dot(direction)
                >= tuning.hand_reach_m
        })
    });
    let bonus = if hands_reaching {
        tuning.hand_lean_m.max(0.0)
    } else {
        0.0
    };
    let radius = lean_radius(along, tuning);
    let lean_reach = (arc.max(radius) + bonus).min(tuning.max_lean_m.max(radius + bonus));
    let excess = (reach - lean_reach).max(0.0);

    // The bend drawn: the spine tilted toward the neck as far as the offset and the limit allow.
    let shown = (reach / spine).min(limit.sin()).clamp(0.0, 1.0).asin();
    let bend = Bend {
        pitch: shown * along,
        roll: shown * across,
    };
    let waist_y = (neck.y - spine * shown.cos()).min(input.hip_height_m);
    let solve = PelvisSolve {
        neck,
        waist: Vec3::new(input.pelvis.x, waist_y, input.pelvis.y),
        spine_m: spine,
        offset,
        bend,
        lean_reach_m: lean_reach,
        step_demand: direction * excess,
        drift: offset - body_forward * offset.dot(body_forward).max(0.0),
        crouch_m: (input.hip_height_m - waist_y).max(0.0),
        hands_reaching,
    };
    if solve.step_demand.is_finite() && solve.drift.is_finite() && solve.waist.is_finite() {
        solve
    } else {
        PelvisSolve::default()
    }
}

/// The floor under the reach a bend explains toward a direction `along` the body's forward (a unit
/// vector's part): the lean radius ahead and to the side, narrowing to the back radius straight
/// back.
fn lean_radius(along: f32, tuning: &PelvisTuning) -> f32 {
    let ahead = tuning.lean_radius_m.max(0.0);
    let behind = tuning.back_radius_m.max(0.0).min(ahead);
    let back = (-along).clamp(0.0, 1.0);
    ahead + (behind - ahead) * back
}

/// The furthest the spine bends toward a direction `along` the body's forward and `across` toward
/// its right (a unit vector's parts), radians: an ellipse through the forward or back limit and the
/// side limit, below a right angle.
fn bend_limit(along: f32, across: f32, tuning: &PelvisTuning) -> f32 {
    let lengthwise = if along >= 0.0 {
        tuning.forward_bend_deg
    } else {
        tuning.back_bend_deg
    };
    let clamp = |deg: f32| deg.clamp(1.0, 89.0).to_radians();
    let (a, b) = (clamp(lengthwise), clamp(tuning.side_bend_deg));
    let limit = 1.0 / ((along / a).powi(2) + (across / b).powi(2)).sqrt();
    if limit.is_finite() {
        limit.min(a.max(b))
    } else {
        a
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use test_case::test_case;

    use super::*;
    use crate::body::EYES_TO_NECK;

    const TUNING: PelvisTuning = PelvisTuning::new();
    const STANDING_NECK: f32 = 1.6;
    const HIP: f32 = 0.95;

    /// The input for a neck pivot at `neck` with a level head facing -Z, over a pelvis at the
    /// origin, with the hands down.
    fn at_neck(neck: Vec3) -> PelvisInput {
        PelvisInput {
            eyes: neck - EYES_TO_NECK,
            head: Quat::IDENTITY,
            pelvis: Vec2::ZERO,
            body_yaw: 0.0,
            hands: [None, None],
            standing_neck_m: STANDING_NECK,
            hip_height_m: HIP,
        }
    }

    /// The neck carried out along `direction` by a bend of `tilt` radians about the waist.
    fn bent(direction: Vec2, tilt: f32) -> Vec3 {
        let spine = STANDING_NECK - HIP;
        let out = direction * spine * tilt.sin();
        Vec3::new(out.x, HIP + spine * tilt.cos(), out.y)
    }

    #[test]
    fn standing_upright_is_neither_a_lean_nor_a_step() {
        let solve = solve(&at_neck(Vec3::new(0.0, STANDING_NECK, 0.0)), &TUNING);
        assert!(solve.offset.length() < 1e-5);
        assert_eq!(solve.step_demand, Vec2::ZERO);
        assert!(solve.bend.angle() < 1e-3);
        assert!(solve.crouch_m < 1e-4);
    }

    #[test]
    fn a_turn_or_a_nod_moves_nothing_below_the_neck() {
        let neck = Vec3::new(0.0, STANDING_NECK, 0.0);
        for head in [
            Quat::from_rotation_y(1.2),
            Quat::from_rotation_x(-1.0),
            Quat::from_rotation_y(-0.8) * Quat::from_rotation_x(0.5),
        ] {
            let mut input = at_neck(neck);
            input.head = head;
            input.eyes = neck - head * EYES_TO_NECK;
            let solve = solve(&input, &TUNING);
            assert!(solve.offset.length() < 1e-5);
            assert_eq!(solve.step_demand, Vec2::ZERO);
        }
    }

    #[test_case(Vec2::new(0.0, -1.0), 50.0; "forward")]
    #[test_case(Vec2::new(1.0, 0.0), 25.0; "to the right")]
    #[test_case(Vec2::new(0.0, 1.0), 8.0; "back")]
    fn a_bend_on_the_arc_within_its_limit_is_a_lean(direction: Vec2, tilt_deg: f32) {
        let solve = solve(&at_neck(bent(direction, tilt_deg.to_radians())), &TUNING);
        assert_eq!(solve.step_demand, Vec2::ZERO, "{solve:?}");
        assert!((solve.bend.angle().to_degrees() - tilt_deg).abs() < 0.5);
        assert!(solve.crouch_m < 1e-3);
    }

    #[test]
    fn a_forward_bend_pitches_and_a_side_bend_rolls() {
        let forward = solve(&at_neck(bent(Vec2::new(0.0, -1.0), 0.6)), &TUNING);
        assert!(forward.bend.pitch > 0.55 && forward.bend.roll.abs() < 1e-4);
        let side = solve(&at_neck(bent(Vec2::new(1.0, 0.0), 0.4)), &TUNING);
        assert!(side.bend.roll > 0.35 && side.bend.pitch.abs() < 1e-4);
    }

    #[test]
    fn a_level_move_past_the_radius_is_a_step() {
        let solve = solve(&at_neck(Vec3::new(0.0, STANDING_NECK, -0.5)), &TUNING);
        assert!((solve.demand_m() - (0.5 - TUNING.lean_radius_m)).abs() < 1e-4);
        assert!(solve.step_demand.y < 0.0);
    }

    #[test]
    fn a_level_move_inside_the_radius_is_absorbed() {
        let solve = solve(&at_neck(Vec3::new(0.15, STANDING_NECK, 0.0)), &TUNING);
        assert_eq!(solve.step_demand, Vec2::ZERO);
        assert!(solve.bend.angle() > 0.0);
    }

    #[test]
    fn a_bend_past_its_limit_steps_the_excess() {
        // To the side a bend of 45 degrees explains only 30.
        let solve = solve(
            &at_neck(bent(Vec2::new(1.0, 0.0), 45f32.to_radians())),
            &TUNING,
        );
        let spine = STANDING_NECK - HIP;
        let expected = spine * 45f32.to_radians().sin()
            - (spine * 30f32.to_radians().sin()).max(TUNING.lean_radius_m);
        assert!((solve.demand_m() - expected).abs() < 1e-3, "{solve:?}");
    }

    #[test]
    fn stepping_back_steps_sooner_than_stepping_forward() {
        let back = solve(&at_neck(Vec3::new(0.0, STANDING_NECK, 0.18)), &TUNING);
        assert!((back.demand_m() - (0.18 - TUNING.back_radius_m)).abs() < 1e-4);
        let ahead = solve(&at_neck(Vec3::new(0.0, STANDING_NECK, -0.18)), &TUNING);
        assert_eq!(ahead.step_demand, Vec2::ZERO);
    }

    #[test]
    fn the_drift_is_the_side_and_back_of_the_offset() {
        let ahead = solve(&at_neck(Vec3::new(0.0, STANDING_NECK, -0.15)), &TUNING);
        assert!(ahead.drift.length() < 1e-5);
        let side = solve(&at_neck(Vec3::new(0.1, STANDING_NECK, -0.1)), &TUNING);
        assert!(
            (side.drift - Vec2::new(0.1, 0.0)).length() < 1e-5,
            "{side:?}"
        );
        let back = solve(&at_neck(Vec3::new(0.0, STANDING_NECK, 0.1)), &TUNING);
        assert!((back.drift - Vec2::new(0.0, 0.1)).length() < 1e-5);
    }

    #[test]
    fn a_straight_drop_is_a_crouch() {
        let solve = solve(&at_neck(Vec3::new(0.0, STANDING_NECK - 0.4, 0.0)), &TUNING);
        assert!((solve.crouch_m - 0.4).abs() < 1e-4);
        assert_eq!(solve.step_demand, Vec2::ZERO);
    }

    #[test]
    fn both_hands_reaching_along_the_offset_widen_the_lean() {
        let neck = Vec3::new(0.0, STANDING_NECK, -0.25);
        let alone = solve(&at_neck(neck), &TUNING);
        let mut input = at_neck(neck);
        input.hands = [
            Some(neck + Vec3::new(-0.2, -0.2, -0.4)),
            Some(neck + Vec3::new(0.2, -0.2, -0.4)),
        ];
        let reaching = solve(&input, &TUNING);
        assert!(reaching.hands_reaching);
        assert!(reaching.demand_m() < alone.demand_m());
        // One hand alone does not count.
        input.hands[1] = None;
        assert!(!solve(&input, &TUNING).hands_reaching);
    }

    #[test]
    fn the_lean_is_never_wider_than_its_cap() {
        // A deep crouch explains nearly anything by height; the cap still steps a far neck.
        let solve = solve(&at_neck(Vec3::new(0.0, HIP, -0.9)), &TUNING);
        assert!((solve.lean_reach_m - TUNING.max_lean_m).abs() < 1e-5);
        assert!((solve.demand_m() - (0.9 - TUNING.max_lean_m)).abs() < 1e-4);
    }

    #[test]
    fn garbage_in_leaves_nothing_out() {
        let mut input = at_neck(Vec3::new(0.0, STANDING_NECK, 0.0));
        input.eyes = Vec3::NAN;
        assert_eq!(solve(&input, &TUNING), PelvisSolve::default());
    }

    proptest! {
        #[test]
        fn demand_points_along_the_offset_and_never_exceeds_it(
            x in -1.5f32..1.5, z in -1.5f32..1.5, y in 0.6f32..1.9, yaw in -3.1f32..3.1,
        ) {
            let mut input = at_neck(Vec3::new(x, y, z));
            input.body_yaw = yaw;
            let solve = solve(&input, &TUNING);
            prop_assert!(solve.demand_m() <= solve.offset.length() + 1e-4);
            prop_assert!(solve.step_demand.dot(solve.offset) >= -1e-5);
            prop_assert!(solve.lean_reach_m >= TUNING.back_radius_m - 1e-5);
            prop_assert!(solve.waist.y <= HIP + 1e-5);
            prop_assert!(solve.crouch_m >= 0.0);
        }

        #[test]
        fn lowering_the_neck_never_adds_demand(
            x in -1.0f32..1.0, z in -1.0f32..1.0, y in 0.8f32..1.7, drop in 0.0f32..0.4,
        ) {
            let high = solve(&at_neck(Vec3::new(x, y, z)), &TUNING);
            let low = solve(&at_neck(Vec3::new(x, y - drop, z)), &TUNING);
            prop_assert!(low.demand_m() <= high.demand_m() + 1e-4);
        }
    }
}
