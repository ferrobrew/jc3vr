//! The limbs' twist shared out over their twist bones, so a large twist of a hand or a foot does
//! not wring the skin at one joint (candy-wrapping). Pure, in Rico's model space.
//!
//! The skeleton's twist bones are children of a limb segment that carry part of the skin along it
//! (`df_LeftArmRollA01` under `LeftArm`, `LeftForeArmRoll` under `LeftForeArm`; see
//! `docs/engine/character/skeleton.md`). They are found by name, any child of a limb segment whose
//! name has `roll` in it, so a skeleton with thigh or shin twist bones gets them too. Each takes a
//! share of its segment's twist by where it sits along the segment:
//!
//! - On a distal segment (the forearm, the shin), the twist of the hand or the foot against the
//!   segment, about the segment's length: a twist bone `f` of the way to the wrist turns with `f`
//!   of it, so the twist spreads up the forearm from the wrist.
//! - On a proximal segment (the upper arm, the thigh), the segment's own twist against its parent
//!   (the clavicle, the pelvis): a twist bone `f` of the way down turns back by `1 - f` of it, so
//!   the skin at the shoulder or the hip stays with the body while the limb rolls under it.
//!
//! A forearm can only pronate or supinate so far before the rest comes from the shoulder rolling the
//! upper arm, so a hand turned past [`FOREARM_TWIST_LIMIT`] from the forearm passes
//! [`UPPER_ARM_SHARE`] of the excess up to the upper arm ([`passed_up`]): the upper arm's twist
//! bones take it rising toward the elbow, and the forearm's start from it at the elbow, so the skin
//! turns continuously from the shoulder to the wrist and the forearm carries less of a large turn.
//! The bones of the arm themselves are not turned; the elbow stays where the solve put it.
//!
//! Twists are measured against the reference pose's, so a limb at its reference twist leaves its
//! twist bones at theirs.

use body_rig::rotation::twist_angle;
use glam::{Quat, Vec3};
use smol_str::SmolStr;

use crate::controllers::body::procedural::skeleton::SkeletonSnapshot;

/// The region of the body a segment belongs to, for its weight against the animation.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Region {
    Arms,
    Legs,
}

/// A limb segment with twist bones.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub region: Region,
    /// Whether the twist comes from the child joint (the forearm, the shin) rather than from the
    /// segment against its parent (the upper arm, the thigh).
    pub distal: bool,
    /// The segment's bone index.
    pub bone: usize,
    /// Its parent's index.
    pub parent: usize,
    /// The joint at its end's index: the next segment, the hand, or the foot.
    pub end: usize,
    /// The segment's length axis in its own frame, unit length.
    pub axis: Vec3,
    /// The reference pose's segment against its parent, and its end against it.
    pub reference_local: Quat,
    pub reference_end_local: Quat,
    pub rolls: Vec<Roll>,
}

/// A twist bone on a segment.
#[derive(Debug, Clone, PartialEq)]
pub struct Roll {
    pub bone: usize,
    pub name: SmolStr,
    /// The reference pose's twist bone against its segment.
    pub reference_local: Quat,
    /// How far along the segment it sits, `0..=1`, from the segment's joint to its end.
    pub along: f32,
}

/// The skeleton's twist bones, by segment.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TwistRig {
    pub segments: Vec<Segment>,
}

impl TwistRig {
    /// Find the limb segments and the twist bones on them. A segment missing from the skeleton, or
    /// whose parent or end is not the bone expected, is left out.
    pub fn from_skeleton(skeleton: &SkeletonSnapshot) -> Self {
        let model = skeleton.reference_model_space();
        let find = |name: &str| skeleton.bones.iter().position(|b| b.name == name);
        let mut segments = vec![];
        for (bone, parent, end, distal, region) in SEGMENTS {
            let (Some(bone), Some(parent), Some(end)) = (find(bone), find(parent), find(end))
            else {
                continue;
            };
            if skeleton.bones[bone].parent != Some(parent)
                || skeleton.bones[end].parent != Some(bone)
            {
                continue;
            }
            let (s, p, e) = (model[bone], model[parent], model[end]);
            let span = s.rotation.inverse() * (e.translation - s.translation);
            let length = span.length();
            if !(length > 1e-4 && span.is_finite()) {
                continue;
            }
            let axis = span / length;
            let rolls: Vec<Roll> = skeleton
                .bones
                .iter()
                .enumerate()
                .filter(|(_, b)| b.parent == Some(bone) && b.name.to_lowercase().contains("roll"))
                .map(|(i, b)| Roll {
                    bone: i,
                    name: b.name.clone(),
                    reference_local: (s.rotation.inverse() * model[i].rotation).normalize(),
                    along: ((s.rotation.inverse() * (model[i].translation - s.translation))
                        .dot(axis)
                        / length)
                        .clamp(0.0, 1.0),
                })
                .collect();
            if rolls.is_empty() {
                continue;
            }
            segments.push(Segment {
                region,
                distal,
                bone,
                parent,
                end,
                axis,
                reference_local: (p.rotation.inverse() * s.rotation).normalize(),
                reference_end_local: (s.rotation.inverse() * e.rotation).normalize(),
                rolls,
            });
        }
        Self { segments }
    }

    /// The highest bone index the rig reads or writes.
    pub fn highest_bone(&self) -> usize {
        self.segments
            .iter()
            .flat_map(|s| {
                [s.bone, s.parent, s.end]
                    .into_iter()
                    .chain(s.rolls.iter().map(|r| r.bone))
            })
            .max()
            .unwrap_or(0)
    }
}

impl Segment {
    /// The segment's twist against the reference pose, radians: the end's against the segment on a
    /// distal segment, the segment's against its parent on a proximal one, about the segment's
    /// length. `segment`, `parent`, and `end` are the bones' model-space rotations now.
    pub fn twist(&self, parent: Quat, segment: Quat, end: Quat) -> f32 {
        if self.distal {
            let local = segment.inverse() * end;
            twist_angle(local * self.reference_end_local.inverse(), self.axis)
        } else {
            let local = parent.inverse() * segment;
            // The turn from the reference, in the parent's frame, measured about the segment's
            // length as the parent sees it now.
            let axis = local * self.axis;
            twist_angle(local * self.reference_local.inverse(), axis)
        }
    }

    /// The angle `roll` turns by for the segment's twist `twist`, with `passed` of the forearm's
    /// twist handed to the upper arm ([`passed_up`]). On a distal segment it is `along` of the twist
    /// left after the pass, on top of the pass; on a proximal one it is back by `1 - along` of the
    /// segment's own twist, and forward by `along` of the pass.
    pub fn turn(&self, roll: &Roll, twist: f32, passed: f32) -> f32 {
        if self.distal {
            passed + (twist - passed) * roll.along
        } else {
            -(1.0 - roll.along) * twist + roll.along * passed
        }
    }

    /// Each twist bone's model-space rotation for the segment's twist `twist`, with `passed`
    /// handed up from the forearm ([`Segment::turn`]), the segment turned `segment` in model space,
    /// mixed from each bone's `current` rotation by `weight`.
    pub fn rolls(
        &self,
        segment: Quat,
        twist: f32,
        passed: f32,
        current: &[Quat],
        weight: f32,
    ) -> Vec<Quat> {
        let w = weight.clamp(0.0, 1.0);
        let passed = if passed.is_finite() { passed } else { 0.0 };
        self.rolls
            .iter()
            .zip(current)
            .map(|(roll, now)| {
                let turn = Quat::from_axis_angle(self.axis, self.turn(roll, twist, passed));
                let wanted = (segment * turn * roll.reference_local).normalize();
                let mixed = now.slerp(wanted, w).normalize();
                if mixed.is_finite() { mixed } else { *now }
            })
            .collect()
    }
}

/// How far a hand turns from its forearm before the upper arm takes a share, radians: about the
/// forearm's own range of pronation from the reference.
pub const FOREARM_TWIST_LIMIT: f32 = std::f32::consts::FRAC_PI_2;

/// The share of a hand's turn past [`FOREARM_TWIST_LIMIT`] the upper arm takes.
pub const UPPER_ARM_SHARE: f32 = 0.5;

/// The part of a forearm's twist `twist` handed up to the upper arm: [`UPPER_ARM_SHARE`] of what
/// is past [`FOREARM_TWIST_LIMIT`], in the twist's own direction.
pub fn passed_up(twist: f32) -> f32 {
    if !twist.is_finite() {
        return 0.0;
    }
    (twist.abs() - FOREARM_TWIST_LIMIT).max(0.0) * UPPER_ARM_SHARE * twist.signum()
}

impl TwistRig {
    /// What each segment passes or receives ([`passed_up`]), by segment: a forearm passes its own,
    /// an upper arm receives its forearm's (the distal segment that hangs from it), and the legs
    /// none. `twists` are the segments' twists in order.
    pub fn passes(&self, twists: &[f32]) -> Vec<f32> {
        let twist = |i: usize| twists.get(i).copied().unwrap_or(0.0);
        self.segments
            .iter()
            .enumerate()
            .map(|(i, segment)| match (segment.region, segment.distal) {
                (Region::Legs, _) => 0.0,
                (Region::Arms, true) => passed_up(twist(i)),
                (Region::Arms, false) => self
                    .segments
                    .iter()
                    .position(|s| s.distal && s.parent == segment.bone)
                    .map_or(0.0, |child| passed_up(twist(child))),
            })
            .collect()
    }
}

/// The limb segments: the segment, its parent, its end, whether the twist comes from its end, and
/// its region.
const SEGMENTS: [(&str, &str, &str, bool, Region); 8] = [
    (
        "LeftArm",
        "LeftShoulder",
        "LeftForeArm",
        false,
        Region::Arms,
    ),
    ("LeftForeArm", "LeftArm", "LeftHand", true, Region::Arms),
    (
        "RightArm",
        "RightShoulder",
        "RightForeArm",
        false,
        Region::Arms,
    ),
    ("RightForeArm", "RightArm", "RightHand", true, Region::Arms),
    ("LeftUpLeg", "Pelvis", "LeftLeg", false, Region::Legs),
    ("LeftLeg", "LeftUpLeg", "LeftFoot", true, Region::Legs),
    ("RightUpLeg", "Pelvis", "RightLeg", false, Region::Legs),
    ("RightLeg", "RightUpLeg", "RightFoot", true, Region::Legs),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::procedural::{
        rig::tests::skeleton,
        skeleton::{Bone, Transform},
    };

    /// The test skeleton with a twist bone near each end of the left arm's segments.
    fn with_rolls() -> SkeletonSnapshot {
        let mut snapshot = skeleton();
        let index =
            |s: &SkeletonSnapshot, name: &str| s.bones.iter().position(|b| b.name == name).unwrap();
        let model = snapshot.reference_model_space();
        for (name, parent, end, at) in [
            ("df_LeftArmRollA01", "LeftArm", "LeftForeArm", 0.2),
            ("LeftForeArmRoll", "LeftForeArm", "LeftHand", 0.8),
        ] {
            let (p, e) = (index(&snapshot, parent), index(&snapshot, end));
            let local = (model[e].translation - model[p].translation) * at;
            snapshot.bones.push(Bone {
                name: name.into(),
                parent: Some(p),
                reference: Transform {
                    translation: local,
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            });
        }
        snapshot
    }

    #[test]
    fn the_twist_bones_are_found_on_their_segments() {
        let rig = TwistRig::from_skeleton(&with_rolls());
        assert_eq!(rig.segments.len(), 2);
        let upper = &rig.segments[0];
        assert!(!upper.distal);
        assert_eq!(upper.rolls[0].name, "df_LeftArmRollA01");
        assert!((upper.rolls[0].along - 0.2).abs() < 1e-4);
        let fore = &rig.segments[1];
        assert!(fore.distal);
        assert!((fore.rolls[0].along - 0.8).abs() < 1e-4);
        // A skeleton without twist bones has no segments.
        assert!(TwistRig::from_skeleton(&skeleton()).segments.is_empty());
    }

    #[test]
    fn a_wrist_twist_spreads_up_the_forearm() {
        let rig = TwistRig::from_skeleton(&with_rolls());
        let fore = &rig.segments[1];
        let snapshot = with_rolls();
        let model = snapshot.reference_model_space();
        let segment = model[fore.bone].rotation;
        let parent = model[fore.parent].rotation;
        // The hand turned a right angle about the forearm, the rest as in the reference.
        let hand = segment * Quat::from_axis_angle(fore.axis, 1.5) * fore.reference_end_local;
        let twist = fore.twist(parent, segment, hand);
        assert!((twist - 1.5).abs() < 1e-4, "{twist}");
        let current = [model[fore.rolls[0].bone].rotation];
        let rolls = fore.rolls(segment, twist, 0.0, &current, 1.0);
        let turned = twist_angle(segment.inverse() * rolls[0], fore.axis);
        assert!((turned - 1.5 * 0.8).abs() < 1e-3, "{turned}");
        // At no weight, the twist bone stays as it is.
        assert_eq!(
            fore.rolls(segment, twist, 0.0, &current, 0.0)[0],
            current[0]
        );
        // A bend of the wrist is not a twist.
        let swing = fore.axis.any_orthonormal_vector();
        let bent = segment * Quat::from_axis_angle(swing, 0.8) * fore.reference_end_local;
        assert!(fore.twist(parent, segment, bent).abs() < 1e-4);
    }

    #[test]
    fn an_upper_arm_roll_is_held_back_at_the_shoulder() {
        let rig = TwistRig::from_skeleton(&with_rolls());
        let upper = &rig.segments[0];
        let snapshot = with_rolls();
        let model = snapshot.reference_model_space();
        let parent = model[upper.parent].rotation;
        // The upper arm rolled about its length by a right angle.
        let reference = parent * upper.reference_local;
        let segment = reference * Quat::from_axis_angle(upper.axis, 1.5);
        let twist = upper.twist(parent, segment, Quat::IDENTITY);
        assert!((twist - 1.5).abs() < 1e-3, "{twist}");
        let current = [model[upper.rolls[0].bone].rotation];
        let rolls = upper.rolls(segment, twist, 0.0, &current, 1.0);
        // The twist bone near the shoulder turns back by most of the roll.
        let turned = twist_angle(segment.inverse() * rolls[0], upper.axis);
        assert!((turned + 1.5 * 0.8).abs() < 1e-3, "{turned}");
    }

    #[test]
    fn a_large_hand_turn_passes_part_of_it_to_the_upper_arm() {
        let rig = TwistRig::from_skeleton(&with_rolls());
        let (upper, fore) = (&rig.segments[0], &rig.segments[1]);
        // Within the forearm's range nothing passes.
        assert_eq!(passed_up(1.0), 0.0);
        assert_eq!(rig.passes(&[0.0, 1.0]), vec![0.0, 0.0]);
        // A 130 degree turn passes half of the 40 past the limit.
        let twist = 130f32.to_radians();
        let passed = passed_up(twist);
        assert!((passed - 20f32.to_radians()).abs() < 1e-5);
        assert_eq!(passed_up(-twist), -passed);
        let passes = rig.passes(&[0.0, twist]);
        assert!((passes[0] - passed).abs() < 1e-6 && (passes[1] - passed).abs() < 1e-6);
        // The skin turns continuously: the upper arm's roll at the elbow end and the forearm's at
        // the elbow both take the pass, and the forearm's at the wrist the whole turn.
        let at = |along: f32| Roll {
            along,
            ..fore.rolls[0].clone()
        };
        assert!((fore.turn(&at(0.0), twist, passed) - passed).abs() < 1e-6);
        assert!((fore.turn(&at(1.0), twist, passed) - twist).abs() < 1e-6);
        assert!((upper.turn(&at(1.0), 0.0, passed) - passed).abs() < 1e-6);
        assert!(upper.turn(&at(0.0), 0.0, passed).abs() < 1e-6);
        // The forearm carries less of the turn along its length.
        let spread = fore.turn(&at(1.0), twist, passed) - fore.turn(&at(0.0), twist, passed);
        assert!(spread < twist - 0.3);
    }
}
