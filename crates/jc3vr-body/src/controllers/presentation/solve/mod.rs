//! The render-time re-solve of the upper body, pure: from the pose the skinning draws (the last two
//! ticks' poses interpolated) and the head's and the hands' motion since those ticks, a rigid edit
//! per bone group ([`Group`]) that the draws pre-multiply onto the palette.
//!
//! All of it is in Rico's model space.
//!
//! - The torso bends after the head: the spine joints turn, each by its share (the presentation
//!   tuning's `solve.spine_shares`), so that the head's joint swings toward where the head has moved to, by
//!   the procedural torso's weight ([`chain_swing`]). The bend turns the spine about its base; it
//!   does not lengthen or shorten it.
//! - The head takes the head's whole motion: it turns by the head's turn since the ticks, about its
//!   own joint, and whatever of the head's travel the spine's bend did not cover moves it, up to
//!   `solve.max_head_residual_m`.
//! - Each arm is solved again from its shoulder, carried by the torso's bend, to its hand: two-bone
//!   IK ([`two_bone_elbow`]) with the upper arm's and the forearm's lengths as drawn. An arm the
//!   controllers drive bends toward the elbow pole ([`crate::controllers::presentation::elbow`]),
//!   smoothed across frames, by how much the controllers drive it; any other arm bends toward
//!   where the drawn elbow is (the solve's own elbow, which its hint shaped), or toward the elbow
//!   hint ([`elbow_hint`]) when the drawn arm is straight. The hand takes the hand's correction as
//!   the late update computes it, so the gun, the held items, and the fingers on it stay where they
//!   are drawn, and the forearm swings, stretched if allowed, to meet it.
//!
//! With no motion since the ticks and no elbow pole every edit is the identity, so the layer cannot
//! move a pose the simulation already agrees with; the pole turns only a driven arm's elbow about
//! the line from its shoulder to its hand.

use body_rig::geometry::within_sphere;
use glam::{Mat4, Quat, Vec3};

use crate::controllers::{
    body::procedural::solver::arms::{Side, elbow_hint},
    hands::{palette::forearm_reach, tuning::HANDS},
    presentation::{
        elbow::{self, PoleInput},
        rig::Group,
        tuning::PRESENTATION,
    },
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
/// and the hand (the wrist), and the hand bone's orientation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmJoints {
    pub clavicle: Vec3,
    pub shoulder: Vec3,
    pub elbow: Vec3,
    pub wrist: Vec3,
    pub hand: Quat,
    pub frame: Quat,
}

impl UpperBody {
    /// The joints `t` of the way from `self` to `to`, as the skinning blends its two poses.
    pub fn lerp(&self, to: &Self, t: f32) -> Self {
        let arm = |a: &ArmJoints, b: &ArmJoints| ArmJoints {
            clavicle: a.clavicle.lerp(b.clavicle, t),
            shoulder: a.shoulder.lerp(b.shoulder, t),
            elbow: a.elbow.lerp(b.elbow, t),
            wrist: a.wrist.lerp(b.wrist, t),
            hand: a.hand.normalize().slerp(b.hand.normalize(), t),
            frame: a.frame.normalize().slerp(b.frame.normalize(), t),
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
            && self.arms.iter().all(|a| {
                a.clavicle.is_finite()
                    && a.shoulder.is_finite()
                    && a.elbow.is_finite()
                    && a.wrist.is_finite()
                    && a.hand.is_finite()
                    && a.frame.is_finite()
            })
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
    pub targets: [Option<(Vec3, Quat, f32)>; 2],
    pub shoulders: [Option<Vec3>; 2],
    /// Whether the arms are solved again; without it only the forearm swings to the hand, about the
    /// elbow as the torso carries it.
    pub arms: bool,
    /// Whether the forearm stretches along its length to meet a hand moved off the end of the arm.
    pub stretch_forearm: bool,
    /// The elbow pole's inputs; `None` bends every arm toward its drawn elbow.
    pub elbow: Option<ElbowDrive>,
}

/// The elbow pole's inputs ([`crate::controllers::presentation::elbow`]), model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ElbowDrive {
    /// The player's up, a unit vector.
    pub up: Vec3,
    /// The little finger's pull (the presentation tuning's `elbow.palm_weight`).
    pub palm_weight: f32,
    /// The smoothing's time constant, seconds.
    pub smoothing_s: f32,
    /// The time since the smoothed poles in [`arms`](Self::arms) were solved, seconds.
    pub dt: f32,
    /// Each arm, left then right.
    pub arms: [ArmDrive; 2],
}

/// How the controllers drive one arm, and its smoothed pole from the last frame.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ArmDrive {
    /// How much the controllers drive the arm, `0..=1`: the hand IK's weight. At none the arm bends
    /// toward its drawn elbow, and its smoothing starts again.
    pub weight: f32,
    /// The smoothed pole the last frame solved, a unit vector; `None` starts the smoothing from the
    /// rule's pole.
    pub previous: Option<Vec3>,
}

/// Each group's edit, by [`Group::index`].
pub type Edits = [Mat4; Group::COUNT];

/// The solve's result: the groups' edits, and each arm's elbow pole, left then right, where the
/// controllers drive the arm.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Solved {
    pub edits: Edits,
    pub elbows: [Option<ElbowSolve>; 2],
}

/// One arm's elbow pole as a frame solved it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ElbowSolve {
    /// The smoothed pole, a unit vector square to the shoulder-to-wrist line, which the next frame
    /// smooths from.
    pub pole: Vec3,
    /// How much of it the arm bent toward, `0..=1`.
    pub weight: f32,
    /// The turn about the shoulder-to-wrist line from the drawn elbow to the pole, degrees; `None`
    /// for a straight drawn arm.
    pub swivel_deg: Option<f32>,
}

/// The groups' edits for `body` as drawn, after `motion`.
pub fn solve(body: &UpperBody, motion: &Motion) -> Solved {
    let tuning = &PRESENTATION.get().solve;
    let mut edits = [Mat4::IDENTITY; Group::COUNT];
    let mut elbows = [None; 2];
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
                tuning.spine_shares,
            );
            for (group, edit) in Group::SPINE.into_iter().zip(chain) {
                edits[group.index()] = edit;
            }
        }
        let neck = edits[Group::Neck.index()];
        let turn = rotation_of(&neck).inverse() * rotation_of(&head);
        let placed = neck * about(body.head, turn);
        let residual = (wanted - placed.transform_point3(body.head))
            .clamp_length_max(tuning.max_head_residual_m);
        edits[Group::Head.index()] = Mat4::from_translation(residual) * placed;
    }
    let torso = edits[Group::Sternum.index()];
    for (i, side) in [Side::Left, Side::Right].into_iter().enumerate() {
        let mut hand = motion.hands[i].unwrap_or(Mat4::IDENTITY);
        let arm = &body.arms[i];
        if let Some((target, orientation, weight)) = motion.targets[i]
            && target.is_finite()
            && weight.is_finite()
            && orientation.is_finite()
            && orientation.length_squared() > 1e-8
        {
            let weight = weight.clamp(0.0, 1.0);
            let turn = Quat::IDENTITY.slerp(orientation.normalize() * arm.frame.inverse(), weight);
            hand = about(arm.wrist, turn);
            let wrist = arm.wrist.lerp(target, weight);
            hand = Mat4::from_translation(wrist - hand.transform_point3(arm.wrist)) * hand;
        }
        let root = motion.shoulders[i]
            .filter(|point| point.is_finite())
            .map_or(torso, |point| {
                Mat4::from_translation(point - torso.transform_point3(arm.shoulder)) * torso
            });
        let clavicle = if motion.arms {
            clavicle_edit(root, arm, hand, motion.stretch_forearm)
        } else {
            torso
        };
        let group = if i == 0 {
            Group::LeftShoulder
        } else {
            Group::RightShoulder
        };
        edits[group.index()] = clavicle;
        let [upper, fore, palm] = if motion.arms {
            let rule = motion.elbow.as_ref().and_then(|drive| {
                let arm = drive.arms[i];
                (arm.weight.is_finite() && arm.weight > 0.0).then(|| ElbowRule {
                    drive,
                    arm,
                    other_shoulder: torso.transform_point3(body.arms[1 - i].shoulder),
                    length: body.arms[i].shoulder.distance(body.arms[i].elbow)
                        + body.arms[i].elbow.distance(body.arms[i].wrist),
                })
            });
            let (edits, elbow) = arm_edits(clavicle, arm, hand, motion.stretch_forearm, side, rule);
            elbows[i] = elbow;
            edits
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
    Solved { edits, elbows }
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
    for _ in 0..PRESENTATION.get().solve.chain_iterations {
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

fn clavicle_edit(torso: Mat4, arm: &ArmJoints, hand: Mat4, stretch: bool) -> Mat4 {
    let pivot = torso.transform_point3(arm.clavicle);
    let shoulder = torso.transform_point3(arm.shoulder);
    let target = hand.transform_point3(arm.wrist);
    let reach =
        arm.shoulder.distance(arm.elbow) + arm.elbow.distance(arm.wrist) * stretch_limit(stretch);
    if shoulder.distance(target) <= reach {
        return torso;
    }
    let offset = shoulder - pivot;
    let toward = target - pivot;
    let (Some(from), Some(to)) = (offset.try_normalize(), toward.try_normalize()) else {
        return torso;
    };
    let (length, distance) = (offset.length(), toward.length());
    let needed = ((length * length + distance * distance - reach * reach)
        / (2.0 * length * distance))
        .clamp(-1.0, 1.0)
        .acos();
    let angle = from.angle_between(to);
    let max_turn = PRESENTATION.get().solve.clavicle_max_turn_deg.to_radians();
    let turn = (angle - needed).clamp(0.0, max_turn);
    if angle <= 1e-5 {
        return torso;
    }
    let swing = Quat::IDENTITY.slerp(Quat::from_rotation_arc(from, to), turn / angle);
    about(pivot, swing) * torso
}

/// The upper arm's, the forearm's, and the hand's edits of one arm whose shoulder rides `torso`,
/// solved to the hand moved by `hand`, and its elbow pole where `rule` gives one. A hand further from the shoulder than the arm reaches, with
/// the forearm stretched as far as it may, is drawn at that reach along the line to it: within one
/// tick the body can move against the hands further than the arm covers (a throw and a fall at
/// once), and the hand falling short keeps it on the end of the forearm rather than leaving the
/// forearm behind.
fn arm_edits(
    torso: Mat4,
    arm: &ArmJoints,
    hand: Mat4,
    stretch: bool,
    side: Side,
    rule: Option<ElbowRule>,
) -> ([Mat4; 3], Option<ElbowSolve>) {
    let shoulder = torso.transform_point3(arm.shoulder);
    let carried_elbow = torso.transform_point3(arm.elbow);
    let upper = arm.shoulder.distance(arm.elbow);
    let fore = arm.elbow.distance(arm.wrist);
    let hand = within_reach(
        shoulder,
        hand,
        arm.wrist,
        upper + fore * stretch_limit(stretch),
    );
    let wrist = hand.transform_point3(arm.wrist);
    let reach = wrist - shoulder;
    let drawn = drawn_pole(
        carried_elbow - shoulder,
        torso.transform_point3(arm.wrist) - shoulder,
        reach,
    );
    let ruled = rule
        .and_then(|rule| rule.pole(shoulder, wrist, rotation_of(&hand) * arm.hand, side, drawn));
    let pole = ruled
        .map(|(pole, _)| pole)
        .or_else(|| {
            let axis = reach.try_normalize()?;
            let hint_wrist = shoulder + axis * ((upper + fore) * 0.5);
            let hint = elbow_hint(shoulder, hint_wrist, upper + fore, side, 0.0)? - shoulder;
            let fallback = (hint - axis * hint.dot(axis)).try_normalize()?;
            Some(blend_drawn_pole(drawn, fallback, axis, 1.0))
        })
        .unwrap_or(carried_elbow - shoulder);
    let elbow = two_bone_elbow(shoulder, wrist, upper, fore, pole)
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
    ([upper_edit, fore_edit, hand], ruled.map(|(_, solve)| solve))
}

/// One controller-driven arm's share of the elbow pole's inputs.
#[derive(Debug, Copy, Clone)]
struct ElbowRule<'a> {
    drive: &'a ElbowDrive,
    arm: ArmDrive,
    /// The other arm's shoulder, carried by the torso's edit.
    other_shoulder: Vec3,
    length: f32,
}

impl ElbowRule<'_> {
    /// The pole the arm on `side` from `shoulder` to `wrist`, its hand bone turned `hand`, bends
    /// toward: the rule's pole smoothed from the last frame's, blended from the drawn elbow's
    /// direction `drawn_pole` (from the shoulder) by the arm's weight. `None` for a degenerate arm.
    fn pole(
        &self,
        shoulder: Vec3,
        wrist: Vec3,
        hand: Quat,
        side: Side,
        drawn: Option<DrawnPole>,
    ) -> Option<(Vec3, ElbowSolve)> {
        let axis = (wrist - shoulder).try_normalize()?;
        let input = PoleInput {
            shoulder,
            other_shoulder: self.other_shoulder,
            wrist,
            up: self.drive.up,
            little_finger: elbow::little_finger(hand, side),
        };
        let tuning = &PRESENTATION.get().elbow;
        let extension = shoulder.distance(wrist) / self.length;
        let bend =
            ((tuning.palm_straight_extension - extension) / tuning.palm_fade_span).clamp(0.0, 1.0);
        let bend = bend * bend * (3.0 - 2.0 * bend);
        let target = elbow::pole(&input, side, self.drive.palm_weight * bend)?;
        let smoothed = self.arm.previous.map_or(target, |previous| {
            elbow::smooth(
                previous,
                target,
                axis,
                self.drive.dt,
                self.drive.smoothing_s,
            )
        });
        let weight = self.arm.weight.clamp(0.0, 1.0);
        let pole = blend_drawn_pole(drawn, smoothed, axis, 1.0 - weight);
        let solve = ElbowSolve {
            pole: smoothed,
            weight,
            swivel_deg: drawn
                .filter(|drawn| drawn.confidence > 0.0)
                .map(|drawn| drawn.direction.angle_between(smoothed).to_degrees()),
        };
        Some((pole, solve))
    }
}

#[derive(Debug, Copy, Clone)]
struct DrawnPole {
    direction: Vec3,
    confidence: f32,
}

fn drawn_pole(upper: Vec3, reach: Vec3, target_reach: Vec3) -> Option<DrawnPole> {
    let axis = reach.try_normalize()?;
    let target_axis = target_reach.try_normalize()?;
    let across = upper - axis * upper.dot(axis);
    let bend = across.length() / upper.length();
    let tuning = &PRESENTATION.get().solve;
    let (min, full) = (tuning.min_drawn_bend_sin, tuning.full_drawn_bend_sin);
    let confidence = ((bend - min) / (full - min)).clamp(0.0, 1.0);
    let confidence = confidence * confidence * (3.0 - 2.0 * confidence);
    let direction = Quat::from_rotation_arc(axis, target_axis) * across.try_normalize()?;
    let direction = (direction - target_axis * direction.dot(target_axis)).try_normalize()?;
    Some(DrawnPole {
        direction,
        confidence,
    })
}

fn blend_drawn_pole(drawn: Option<DrawnPole>, fallback: Vec3, axis: Vec3, weight: f32) -> Vec3 {
    let Some(drawn) = drawn else {
        return fallback;
    };
    let weight = drawn.confidence * weight;
    let angle = axis
        .dot(fallback.cross(drawn.direction))
        .atan2(fallback.dot(drawn.direction));
    Quat::from_axis_angle(axis, angle * weight) * fallback
}

/// How far the forearm may reach as a share of its length: its stretch's cap, or its own length.
fn stretch_limit(stretch: bool) -> f32 {
    if stretch {
        HANDS.get().reach.max_forearm_stretch
    } else {
        1.0
    }
}

/// `hand` moved back along the line from `shoulder` so that the wrist it carries from `wrist`
/// (model space, as drawn) is at most `reach` from the shoulder.
fn within_reach(shoulder: Vec3, hand: Mat4, wrist: Vec3, reach: f32) -> Mat4 {
    let moved = hand.transform_point3(wrist);
    if !moved.is_finite() {
        return hand;
    }
    Mat4::from_translation(within_sphere(moved, shoulder, reach) - moved) * hand
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

/// The least the shares' scale-up is taken to be, so a chain folded back on itself is not turned by
/// many times its swing.
const MIN_CHAIN_GAIN: f32 = 0.25;

/// How far the pole must stand off the reach for its side to count, metres.
const POLE_EPSILON_M: f32 = 1e-3;

#[cfg(test)]
mod tests;
