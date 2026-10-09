//! The render-time re-solve of the upper body, pure: from the pose the skinning draws (the last two
//! ticks' poses interpolated) and the head's and the hands' motion since those ticks, a rigid edit
//! per bone group ([`Group`]) that the draws pre-multiply onto the palette.
//!
//! All of it is in Rico's model space.
//!
//! - The torso bends after the head: the spine joints turn, each by its share
//!   ([`SPINE_SHARES`]), so that the head's joint swings toward where the head has moved to, by
//!   the procedural torso's weight ([`chain_swing`]). The bend turns the spine about its base; it
//!   does not lengthen or shorten it.
//! - The head takes the head's whole motion: it turns by the head's turn since the ticks, about its
//!   own joint, and whatever of the head's travel the spine's bend did not cover moves it, up to
//!   [`MAX_HEAD_RESIDUAL_M`].
//! - Each arm is solved again from its shoulder, carried by the torso's bend, to its hand: two-bone
//!   IK ([`two_bone_elbow`]) with the upper arm's and the forearm's lengths as drawn, the elbow
//!   bending toward where the drawn elbow is (the solve's own elbow, which its hint shaped), or
//!   toward the elbow hint ([`elbow_hint`]) when the drawn arm is straight. The hand takes the hand's
//!   correction as the late update computes it, so the gun, the held items, and the fingers on it
//!   stay where they are drawn, and the forearm swings, stretched if allowed, to meet it.
//!
//! With no motion since the ticks every edit is the identity, so the layer cannot move a pose the
//! simulation already agrees with.

use glam::{Mat4, Quat, Vec3};

use crate::controllers::{
    body::procedural::solver::arms::{Side, elbow_hint},
    hands::palette::forearm_reach,
    presentation::rig::Group,
};

/// The upper body's joints as the skinning draws them, model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct UpperBody {
    /// The origins of the spine joints ([`Group::SPINE`]), base first.
    pub spine: [Vec3; 4],
    /// The head bone's origin, where the spine's chain ends.
    pub head: Vec3,
    /// Each arm, left then right.
    pub arms: [ArmJoints; 2],
}

/// One arm's joints: the origins of the upper arm (the shoulder joint), the forearm (the elbow),
/// and the hand (the wrist).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmJoints {
    pub shoulder: Vec3,
    pub elbow: Vec3,
    pub wrist: Vec3,
}

impl UpperBody {
    /// The joints `t` of the way from `self` to `to`, as the skinning blends its two poses.
    pub fn lerp(&self, to: &Self, t: f32) -> Self {
        let arm = |a: &ArmJoints, b: &ArmJoints| ArmJoints {
            shoulder: a.shoulder.lerp(b.shoulder, t),
            elbow: a.elbow.lerp(b.elbow, t),
            wrist: a.wrist.lerp(b.wrist, t),
        };
        Self {
            spine: std::array::from_fn(|i| self.spine[i].lerp(to.spine[i], t)),
            head: self.head.lerp(to.head, t),
            arms: [
                arm(&self.arms[0], &to.arms[0]),
                arm(&self.arms[1], &to.arms[1]),
            ],
        }
    }

    pub fn is_finite(&self) -> bool {
        self.spine.iter().all(|p| p.is_finite())
            && self.head.is_finite()
            && self
                .arms
                .iter()
                .all(|a| a.shoulder.is_finite() && a.elbow.is_finite() && a.wrist.is_finite())
    }
}

/// What moved since the ticks the skinning draws, model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Motion {
    /// The head's motion: a rigid transform taking the head as the ticks had it to where it is
    /// now. `None` leaves the torso and the head as drawn.
    pub head: Option<Mat4>,
    /// How much of the head's travel the spine bends after, `0..=1`.
    pub torso: f32,
    /// Each hand's correction, left then right; `None` holds the hand where it is drawn.
    pub hands: [Option<Mat4>; 2],
    /// Whether the arms are solved again; without it only the forearm swings to the hand, about the
    /// elbow as the torso carries it.
    pub arms: bool,
    /// Whether the forearm stretches along its length to meet a hand moved off the end of the arm.
    pub stretch_forearm: bool,
}

/// Each group's edit, by [`Group::index`].
pub type Edits = [Mat4; Group::COUNT];

/// Each spine joint's share of the bend, base first: more of it higher, as the procedural spine
/// shares its own swing.
pub const SPINE_SHARES: [f32; 4] = [0.15, 0.25, 0.3, 0.3];

/// How far the head moves off the end of the bent spine at most, metres: what the bend cannot
/// cover in a frame is a tick of the head's travel at most, and anything more is not the head's
/// motion.
pub const MAX_HEAD_RESIDUAL_M: f32 = 0.05;

/// The groups' edits for `body` as drawn, after `motion`.
pub fn solve(body: &UpperBody, motion: &Motion) -> Edits {
    let mut edits = [Mat4::IDENTITY; Group::COUNT];
    if let Some(head) = motion.head {
        let wanted = head.transform_point3(body.head);
        let torso = if motion.torso.is_finite() {
            motion.torso.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if torso > 0.0 {
            let chain = chain_swing(
                body.spine,
                body.head,
                body.head.lerp(wanted, torso),
                SPINE_SHARES,
            );
            for (group, edit) in Group::SPINE.into_iter().zip(chain) {
                edits[group.index()] = edit;
            }
        }
        let neck = edits[Group::Neck.index()];
        let turn = rotation_of(&neck).inverse() * rotation_of(&head);
        let placed = neck * about(body.head, turn);
        let residual =
            (wanted - placed.transform_point3(body.head)).clamp_length_max(MAX_HEAD_RESIDUAL_M);
        edits[Group::Head.index()] = Mat4::from_translation(residual) * placed;
    }
    let torso = edits[Group::Sternum.index()];
    for (i, side) in [Side::Left, Side::Right].into_iter().enumerate() {
        let hand = motion.hands[i].unwrap_or(Mat4::IDENTITY);
        let arm = &body.arms[i];
        let [upper, fore, palm] = if motion.arms {
            arm_edits(torso, arm, hand, motion.stretch_forearm, side)
        } else {
            let carried_wrist = torso.transform_point3(arm.wrist);
            let elbow = torso.transform_point3(arm.elbow);
            let wrist = hand.transform_point3(arm.wrist);
            [
                torso,
                forearm_reach(elbow, carried_wrist, wrist, motion.stretch_forearm) * torso,
                hand,
            ]
        };
        let [g_upper, g_fore, g_hand] = Group::arm(i);
        edits[g_upper.index()] = upper;
        edits[g_fore.index()] = fore;
        edits[g_hand.index()] = palm;
    }
    edits
}

/// The rigid transform that takes a head whose neck pivot was at `pivot_before` turned
/// `turn_before` to one at `pivot_now` turned `turn_now`: a turn about the pivot and the pivot's
/// travel. The turns are world orientations; only the turn between them is applied.
pub fn head_motion(pivot_before: Vec3, turn_before: Quat, pivot_now: Vec3, turn_now: Quat) -> Mat4 {
    let turn = (turn_now.normalize() * turn_before.normalize().inverse()).normalize();
    Mat4::from_translation(pivot_now)
        * Mat4::from_quat(turn)
        * Mat4::from_translation(-pivot_before)
}

/// Each chain joint's edit, base first, that bends the chain whose joints are at `joints` and that
/// ends at `end` so the end swings onto the line from the base to `target`, each joint taking its
/// share of the swing. The joints turn about their own origins, so the bones keep their lengths and
/// the base stays put; a target off the sphere the chain's end can sweep is reached in direction,
/// not in distance.
pub fn chain_swing<const N: usize>(
    joints: [Vec3; N],
    end: Vec3,
    target: Vec3,
    shares: [f32; N],
) -> [Mat4; N] {
    let mut turns = [Quat::IDENTITY; N];
    let Some(&base) = joints.first() else {
        return [Mat4::IDENTITY; N];
    };
    for _ in 0..CHAIN_ITERATIONS {
        let edits = chain_edits(&joints, &turns);
        let reached = edits[N - 1].transform_point3(end);
        let (Some(from), Some(to)) = (
            (reached - base).try_normalize(),
            (target - base).try_normalize(),
        ) else {
            break;
        };
        let swing = Quat::from_rotation_arc(from, to);
        if swing.angle_between(Quat::IDENTITY) < 1e-7 {
            break;
        }
        // A joint's turn swings the end by its lever, the joint's distance to the end, against the
        // base's: the shares are scaled up by how much less than the base the joints carry the end
        // together, so the pass covers the swing rather than a share of it.
        let carried = |i: usize| {
            if i == 0 {
                base
            } else {
                edits[i - 1].transform_point3(joints[i])
            }
        };
        let reach = (reached - base).length();
        let gain = (0..N)
            .map(|i| shares[i].clamp(0.0, 1.0) * carried(i).distance(reached) / reach)
            .sum::<f32>()
            .max(MIN_CHAIN_GAIN);
        for i in 0..N {
            // A joint's turn is in the chain's frame before the edits, so the swing, which is in
            // the frame after the joints below have turned, is brought back through their turn.
            let below = if i == 0 {
                Quat::IDENTITY
            } else {
                rotation_of(&edits[i - 1])
            };
            let part = Quat::IDENTITY.slerp(swing, (shares[i].clamp(0.0, 1.0) / gain).min(1.0));
            turns[i] = (below.inverse() * part * below * turns[i]).normalize();
        }
    }
    chain_edits(&joints, &turns)
}

/// The elbow of an arm whose upper arm is `upper` long and forearm `fore` long, reaching from
/// `shoulder` toward `wrist`, bent toward `pole` (a direction from the shoulder). A wrist out of
/// reach straightens the arm toward it, and one nearer than the arm folds to is held at that.
/// `None` for a degenerate arm or a pole along the reach.
pub fn two_bone_elbow(
    shoulder: Vec3,
    wrist: Vec3,
    upper: f32,
    fore: f32,
    pole: Vec3,
) -> Option<Vec3> {
    let reach = wrist - shoulder;
    let axis = reach.try_normalize()?;
    if !(upper > 1e-4 && fore > 1e-4 && pole.is_finite()) {
        return None;
    }
    let side = pole - axis * pole.dot(axis);
    if side.length() < POLE_EPSILON_M {
        return None;
    }
    let side = side.normalize();
    let shortest = (upper - fore).abs() + 1e-4;
    let longest = upper + fore - 1e-4;
    let distance = reach.length().clamp(shortest.min(longest), longest);
    let cos = ((upper * upper + distance * distance - fore * fore) / (2.0 * upper * distance))
        .clamp(-1.0, 1.0);
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    Some(shoulder + axis * (upper * cos) + side * (upper * sin))
}

/// The upper arm's, the forearm's, and the hand's edits of one arm whose shoulder rides `torso`,
/// solved to the hand moved by `hand`.
fn arm_edits(torso: Mat4, arm: &ArmJoints, hand: Mat4, stretch: bool, side: Side) -> [Mat4; 3] {
    let shoulder = torso.transform_point3(arm.shoulder);
    let carried_elbow = torso.transform_point3(arm.elbow);
    let wrist = hand.transform_point3(arm.wrist);
    let upper = arm.shoulder.distance(arm.elbow);
    let fore = arm.elbow.distance(arm.wrist);
    let elbow = two_bone_elbow(shoulder, wrist, upper, fore, carried_elbow - shoulder)
        .or_else(|| {
            let hint = elbow_hint(shoulder, wrist, upper + fore, side, 0.0)?;
            two_bone_elbow(shoulder, wrist, upper, fore, hint - shoulder)
        })
        .filter(|e| e.is_finite())
        .unwrap_or(carried_elbow);
    let swing = match (
        (carried_elbow - shoulder).try_normalize(),
        (elbow - shoulder).try_normalize(),
    ) {
        (Some(from), Some(to)) => Quat::from_rotation_arc(from, to),
        _ => Quat::IDENTITY,
    };
    let upper_edit = about(shoulder, swing) * torso;
    let carried_wrist = upper_edit.transform_point3(arm.wrist);
    let fore_edit = forearm_reach(elbow, carried_wrist, wrist, stretch) * upper_edit;
    [upper_edit, fore_edit, hand]
}

/// The chain's joints' edits for each joint turned by `turns` about its own origin, each carried by
/// the joints below it.
fn chain_edits<const N: usize>(joints: &[Vec3; N], turns: &[Quat; N]) -> [Mat4; N] {
    let mut edits = [Mat4::IDENTITY; N];
    let mut below = Mat4::IDENTITY;
    for i in 0..N {
        below *= about(joints[i], turns[i]);
        edits[i] = below;
    }
    edits
}

/// A turn by `turn` about `pivot`.
fn about(pivot: Vec3, turn: Quat) -> Mat4 {
    Mat4::from_translation(pivot) * Mat4::from_quat(turn) * Mat4::from_translation(-pivot)
}

/// The rotation of a rigid transform.
fn rotation_of(m: &Mat4) -> Quat {
    Quat::from_mat4(m).normalize()
}

/// How many times the chain's swing is shared out at most; each pass leaves a small fraction of the
/// last pass's miss.
const CHAIN_ITERATIONS: usize = 6;

/// The least the shares' scale-up is taken to be, so a chain folded back on itself is not turned by
/// many times its swing.
const MIN_CHAIN_GAIN: f32 = 0.25;

/// How far the pole must stand off the reach for its side to count, metres.
const POLE_EPSILON_M: f32 = 1e-3;

#[cfg(test)]
mod tests {
    use super::*;

    fn arm(side: f32) -> ArmJoints {
        ArmJoints {
            shoulder: Vec3::new(0.18 * side, 1.45, 0.0),
            elbow: Vec3::new(0.22 * side, 1.18, 0.05),
            wrist: Vec3::new(0.22 * side, 1.0, -0.15),
        }
    }

    fn body() -> UpperBody {
        UpperBody {
            spine: [
                Vec3::new(0.0, 1.05, 0.0),
                Vec3::new(0.0, 1.17, 0.0),
                Vec3::new(0.0, 1.3, 0.01),
                Vec3::new(0.0, 1.5, 0.0),
            ],
            head: Vec3::new(0.0, 1.6, -0.01),
            arms: [arm(-1.0), arm(1.0)],
        }
    }

    fn still() -> Motion {
        Motion {
            head: None,
            torso: 1.0,
            hands: [None; 2],
            arms: true,
            stretch_forearm: true,
        }
    }

    fn rigid(m: &Mat4) -> bool {
        let (scale, _, _) = m.to_scale_rotation_translation();
        scale.abs_diff_eq(Vec3::ONE, 1e-4)
    }

    #[test]
    fn with_nothing_moved_every_edit_is_the_identity() {
        for edit in solve(&body(), &still()) {
            assert!(edit.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{edit:?}");
        }
        let unmoved_head = Motion {
            head: Some(Mat4::IDENTITY),
            ..still()
        };
        for edit in solve(&body(), &unmoved_head) {
            assert!(edit.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{edit:?}");
        }
    }

    #[test]
    fn the_chain_swings_its_end_onto_the_target_and_keeps_its_base_and_lengths() {
        let b = body();
        let target = b.head + Vec3::new(0.0, 0.0, -0.04);
        let edits = chain_swing(b.spine, b.head, target, SPINE_SHARES);
        let base = b.spine[0];
        assert!(edits[0].transform_point3(base).abs_diff_eq(base, 1e-6));
        let end = edits[3].transform_point3(b.head);
        let along = (target - base).normalize();
        assert!((end - base).normalize().abs_diff_eq(along, 1e-4), "{end:?}");
        // Every bone keeps its length: each joint carried by the edit below it, and the end.
        let mut joints: Vec<Vec3> = (0..4)
            .map(|i| {
                if i == 0 {
                    base
                } else {
                    edits[i - 1].transform_point3(b.spine[i])
                }
            })
            .collect();
        joints.push(end);
        let mut original = b.spine.to_vec();
        original.push(b.head);
        for i in 0..4 {
            let before = original[i].distance(original[i + 1]);
            let after = joints[i].distance(joints[i + 1]);
            assert!((before - after).abs() < 1e-5);
        }
        // Each joint turns, the higher ones by more of the swing.
        let angle = |m: &Mat4| rotation_of(m).angle_between(Quat::IDENTITY);
        assert!(angle(&edits[0]) > 0.0);
        assert!(angle(&edits[3]) > angle(&edits[0]));
    }

    #[test]
    fn the_head_moves_with_the_spine_and_turns_by_the_head_s_turn() {
        let b = body();
        let turn = Quat::from_rotation_y(0.3);
        let pivot = b.head + Vec3::new(0.0, 0.02, 0.03);
        let head = head_motion(
            pivot,
            Quat::IDENTITY,
            pivot + Vec3::new(0.02, 0.0, -0.03),
            turn,
        );
        let motion = Motion {
            head: Some(head),
            ..still()
        };
        let edits = solve(&b, &motion);
        let wanted = head.transform_point3(b.head);
        let drawn = edits[Group::Head.index()].transform_point3(b.head);
        assert!(drawn.abs_diff_eq(wanted, 1e-4), "{drawn:?} {wanted:?}");
        let rotation = rotation_of(&edits[Group::Head.index()]);
        assert!(rotation.angle_between(turn) < 1e-4);
        // The spine bent toward it: the neck moved, and the base did not.
        let neck = edits[Group::Sternum.index()].transform_point3(b.spine[3]);
        assert!(neck.distance(b.spine[3]) > 1e-3);
        assert!(
            edits[Group::Spine1.index()]
                .transform_point3(b.spine[0])
                .abs_diff_eq(b.spine[0], 1e-6)
        );
        for edit in &edits {
            assert!(rigid(edit));
        }
    }

    #[test]
    fn without_the_torso_the_spine_stays_and_the_head_s_travel_is_capped() {
        let b = body();
        let head = Mat4::from_translation(Vec3::new(0.0, 0.0, -0.2));
        let motion = Motion {
            head: Some(head),
            torso: 0.0,
            ..still()
        };
        let edits = solve(&b, &motion);
        for group in Group::SPINE {
            assert!(edits[group.index()].abs_diff_eq(Mat4::IDENTITY, 1e-6));
        }
        let moved = edits[Group::Head.index()].transform_point3(b.head) - b.head;
        assert!((moved.length() - MAX_HEAD_RESIDUAL_M).abs() < 1e-5);
    }

    #[test]
    fn the_two_bone_elbow_keeps_the_lengths_and_reproduces_the_drawn_arm() {
        let a = arm(1.0);
        let upper = a.shoulder.distance(a.elbow);
        let fore = a.elbow.distance(a.wrist);
        let elbow = two_bone_elbow(a.shoulder, a.wrist, upper, fore, a.elbow - a.shoulder).unwrap();
        assert!(elbow.abs_diff_eq(a.elbow, 1e-5), "{elbow:?}");
        let wrist = a.wrist + Vec3::new(0.05, 0.08, -0.03);
        let elbow = two_bone_elbow(a.shoulder, wrist, upper, fore, a.elbow - a.shoulder).unwrap();
        assert!((elbow.distance(a.shoulder) - upper).abs() < 1e-5);
        assert!((elbow.distance(wrist) - fore).abs() < 1e-4);
        // Out of reach, the arm straightens toward the wrist.
        let far = a.shoulder + Vec3::new(0.0, 0.0, -2.0);
        let elbow = two_bone_elbow(a.shoulder, far, upper, fore, a.elbow - a.shoulder).unwrap();
        let toward = (far - a.shoulder).normalize();
        assert!((elbow - a.shoulder).normalize().dot(toward) > 0.999);
        // A pole along the reach decides nothing.
        assert!(two_bone_elbow(a.shoulder, wrist, upper, fore, wrist - a.shoulder).is_none());
    }

    #[test]
    fn a_moved_hand_is_met_by_a_resolved_arm_of_the_same_lengths() {
        let b = body();
        let hand = Mat4::from_rotation_translation(
            Quat::from_rotation_x(0.2),
            Vec3::new(0.03, 0.06, -0.04),
        );
        let motion = Motion {
            hands: [None, Some(hand)],
            ..still()
        };
        let edits = solve(&b, &motion);
        let a = &b.arms[1];
        let [upper, fore, palm] = Group::arm(1).map(|g| edits[g.index()]);
        assert_eq!(palm, hand);
        let shoulder = upper.transform_point3(a.shoulder);
        let elbow = upper.transform_point3(a.elbow);
        let forearm_elbow = fore.transform_point3(a.elbow);
        let forearm_wrist = fore.transform_point3(a.wrist);
        let wrist = hand.transform_point3(a.wrist);
        assert!(shoulder.abs_diff_eq(a.shoulder, 1e-5));
        assert!(elbow.abs_diff_eq(forearm_elbow, 1e-4));
        assert!(
            forearm_wrist.abs_diff_eq(wrist, 1e-4),
            "{forearm_wrist:?} {wrist:?}"
        );
        assert!((elbow.distance(shoulder) - a.shoulder.distance(a.elbow)).abs() < 1e-5);
        // The other arm, its hand unmoved, is left alone.
        for group in Group::arm(0) {
            assert!(edits[group.index()].abs_diff_eq(Mat4::IDENTITY, 1e-5));
        }
    }

    #[test]
    fn a_hand_held_still_while_the_torso_bends_keeps_its_place_and_its_arm_s_lengths() {
        let b = body();
        let motion = Motion {
            head: Some(Mat4::from_translation(Vec3::new(0.0, 0.0, -0.04))),
            ..still()
        };
        let edits = solve(&b, &motion);
        let a = &b.arms[0];
        let [upper, fore, palm] = Group::arm(0).map(|g| edits[g.index()]);
        assert_eq!(palm, Mat4::IDENTITY);
        let shoulder = upper.transform_point3(a.shoulder);
        assert!(
            shoulder.distance(a.shoulder) > 1e-3,
            "the shoulder rides the torso"
        );
        assert!(
            edits[Group::Sternum.index()]
                .transform_point3(a.shoulder)
                .abs_diff_eq(shoulder, 1e-5)
        );
        let elbow = upper.transform_point3(a.elbow);
        assert!((elbow.distance(shoulder) - a.shoulder.distance(a.elbow)).abs() < 1e-5);
        assert!(fore.transform_point3(a.wrist).abs_diff_eq(a.wrist, 1e-4));
    }

    #[test]
    fn without_the_arm_solve_the_forearm_swings_about_the_carried_elbow() {
        let b = body();
        let hand = Mat4::from_translation(Vec3::new(0.0, 0.03, 0.0));
        let motion = Motion {
            hands: [Some(hand), None],
            arms: false,
            stretch_forearm: false,
            ..still()
        };
        let edits = solve(&b, &motion);
        let a = &b.arms[0];
        assert!(edits[Group::LeftArm.index()].abs_diff_eq(Mat4::IDENTITY, 1e-6));
        let fore = edits[Group::LeftForeArm.index()];
        assert!(fore.transform_point3(a.elbow).abs_diff_eq(a.elbow, 1e-6));
        let swung = fore.transform_point3(a.wrist) - a.elbow;
        let toward = (hand.transform_point3(a.wrist) - a.elbow).normalize();
        assert!(swung.normalize().abs_diff_eq(toward, 1e-5));
    }

    #[test]
    fn the_interpolated_body_blends_every_joint() {
        let a = body();
        let mut b = body();
        b.head += Vec3::Y * 0.1;
        b.arms[1].wrist += Vec3::X * 0.2;
        let half = a.lerp(&b, 0.5);
        assert!((half.head.y - (a.head.y + 0.05)).abs() < 1e-6);
        assert!((half.arms[1].wrist.x - (a.arms[1].wrist.x + 0.1)).abs() < 1e-6);
        assert!(half.is_finite());
    }
}
