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
//! Twists are measured against the reference pose's, so a limb at its reference twist leaves its
//! twist bones at theirs.

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
            twist_about(local * self.reference_end_local.inverse(), self.axis)
        } else {
            let local = parent.inverse() * segment;
            // The turn from the reference, in the parent's frame, measured about the segment's
            // length as the parent sees it now.
            let axis = local * self.axis;
            twist_about(local * self.reference_local.inverse(), axis)
        }
    }

    /// The share of the segment's twist `roll` turns by: `along` of it on a distal segment, and
    /// back by `1 - along` of it on a proximal one.
    pub fn share(&self, roll: &Roll) -> f32 {
        if self.distal {
            roll.along
        } else {
            -(1.0 - roll.along)
        }
    }

    /// Each twist bone's model-space rotation for the segment's twist `twist`, the segment turned
    /// `segment` in model space, mixed from each bone's `current` rotation by `weight`.
    pub fn rolls(&self, segment: Quat, twist: f32, current: &[Quat], weight: f32) -> Vec<Quat> {
        let w = weight.clamp(0.0, 1.0);
        self.rolls
            .iter()
            .zip(current)
            .map(|(roll, now)| {
                let turn = Quat::from_axis_angle(self.axis, twist * self.share(roll));
                let wanted = (segment * turn * roll.reference_local).normalize();
                let mixed = now.slerp(wanted, w).normalize();
                if mixed.is_finite() { mixed } else { *now }
            })
            .collect()
    }
}

/// The twist part of `rotation` about the unit `axis`, radians in `-π..=π`: the angle of its
/// swing-twist split's twist.
pub fn twist_about(rotation: Quat, axis: Vec3) -> f32 {
    let along = Vec3::new(rotation.x, rotation.y, rotation.z).dot(axis);
    let angle = 2.0 * along.atan2(rotation.w);
    let pi = std::f32::consts::PI;
    let wrapped = (angle + pi).rem_euclid(2.0 * pi) - pi;
    if wrapped.is_finite() { wrapped } else { 0.0 }
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
        let rolls = fore.rolls(segment, twist, &current, 1.0);
        let turned = twist_about(segment.inverse() * rolls[0], fore.axis);
        assert!((turned - 1.5 * 0.8).abs() < 1e-3, "{turned}");
        // At no weight, the twist bone stays as it is.
        assert_eq!(fore.rolls(segment, twist, &current, 0.0)[0], current[0]);
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
        let rolls = upper.rolls(segment, twist, &current, 1.0);
        // The twist bone near the shoulder turns back by most of the roll.
        let turned = twist_about(segment.inverse() * rolls[0], upper.axis);
        assert!((turned + 1.5 * 0.8).abs() < 1e-3, "{turned}");
    }

    #[test]
    fn the_twist_is_wrapped_and_finite() {
        let axis = Vec3::Y;
        assert!((twist_about(Quat::from_rotation_y(0.5), axis) - 0.5).abs() < 1e-5);
        assert!((twist_about(Quat::from_rotation_y(-3.0), axis) + 3.0).abs() < 1e-4);
        assert_eq!(twist_about(Quat::from_rotation_x(0.7), axis), 0.0);
        assert_eq!(
            twist_about(Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0), axis),
            0.0
        );
    }
}
