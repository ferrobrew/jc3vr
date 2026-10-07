//! The procedural body's rig: the bones it drives, found by name in the character's skeleton, and
//! their reference pose in model space. Pure.
//!
//! The bones form two chains under `Hips`: the spine (`Spine` to `Head`) and the legs, which hang
//! from `Pelvis`, a sibling of the spine (see `docs/engine/character/skeleton.md`). The solver moves
//! a bone together with everything under it, so the rig insists that each bone's parent in the
//! skeleton is the one it expects: a skeleton with a bone in between would make the solver's
//! propagation wrong, and the rig refuses it instead.
//!
//! The arms are not driven, but the clavicles turn toward the hands, so the rig also measures each
//! arm from its reference pose ([`ArmRig`]): where the shoulder joint sits on its clavicle, and the
//! arm's length to the wrist.

use std::fmt;

use glam::Vec3;

use crate::controllers::body::procedural::{
    skeleton::{SkeletonSnapshot, Transform},
    solver::arms::Side,
};

/// A bone the procedural body drives.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RigBone {
    Hips,
    Pelvis,
    LeftUpLeg,
    LeftLeg,
    LeftFoot,
    RightUpLeg,
    RightLeg,
    RightFoot,
    Spine,
    Spine1,
    Spine2,
    Sternum,
    Neck,
    Head,
    LeftShoulder,
    RightShoulder,
}

impl RigBone {
    /// Every bone, parents before children: the order the pose is written in.
    pub const ALL: [Self; 16] = [
        Self::Hips,
        Self::Pelvis,
        Self::LeftUpLeg,
        Self::LeftLeg,
        Self::LeftFoot,
        Self::RightUpLeg,
        Self::RightLeg,
        Self::RightFoot,
        Self::Spine,
        Self::Spine1,
        Self::Spine2,
        Self::Sternum,
        Self::Neck,
        Self::Head,
        Self::LeftShoulder,
        Self::RightShoulder,
    ];

    pub const COUNT: usize = Self::ALL.len();

    pub const fn index(self) -> usize {
        self as usize
    }

    /// The bone's name in the skeleton.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hips => "Hips",
            Self::Pelvis => "Pelvis",
            Self::LeftUpLeg => "LeftUpLeg",
            Self::LeftLeg => "LeftLeg",
            Self::LeftFoot => "LeftFoot",
            Self::RightUpLeg => "RightUpLeg",
            Self::RightLeg => "RightLeg",
            Self::RightFoot => "RightFoot",
            Self::Spine => "Spine",
            Self::Spine1 => "Spine1",
            Self::Spine2 => "Spine2",
            Self::Sternum => "Sternum",
            Self::Neck => "Neck",
            Self::Head => "Head",
            Self::LeftShoulder => "LeftShoulder",
            Self::RightShoulder => "RightShoulder",
        }
    }

    /// The bone's parent within the rig; `None` for the rig's root.
    pub const fn parent(self) -> Option<Self> {
        match self {
            Self::Hips => None,
            Self::Pelvis | Self::Spine => Some(Self::Hips),
            Self::LeftUpLeg | Self::RightUpLeg => Some(Self::Pelvis),
            Self::LeftLeg => Some(Self::LeftUpLeg),
            Self::LeftFoot => Some(Self::LeftLeg),
            Self::RightLeg => Some(Self::RightUpLeg),
            Self::RightFoot => Some(Self::RightLeg),
            Self::Spine1 => Some(Self::Spine),
            Self::Spine2 => Some(Self::Spine1),
            Self::Sternum => Some(Self::Spine2),
            Self::Neck | Self::LeftShoulder | Self::RightShoulder => Some(Self::Sternum),
            Self::Head => Some(Self::Neck),
        }
    }

    /// Whether `ancestor` is this bone or one of its ancestors in the rig.
    pub fn descends_from(self, ancestor: Self) -> bool {
        let mut bone = Some(self);
        while let Some(b) = bone {
            if b == ancestor {
                return true;
            }
            bone = b.parent();
        }
        false
    }
}

/// The rig of one skeleton.
#[derive(Debug, Clone, PartialEq)]
pub struct Rig {
    /// Each rig bone's index in the skeleton, by [`RigBone::index`].
    pub bones: [usize; RigBone::COUNT],
    /// Each rig bone's reference pose in model space.
    pub reference: [Transform; RigBone::COUNT],
    /// The arms, left then right.
    pub arms: [ArmRig; 2],
}

/// One arm, measured from the reference pose.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmRig {
    /// The shoulder joint (the upper arm's origin) in its clavicle's frame.
    pub shoulder_local: Vec3,
    /// The upper arm's and the forearm's lengths together, metres.
    pub length: f32,
}

impl Rig {
    /// Find the rig's bones in `skeleton` and check its hierarchy.
    pub fn from_skeleton(skeleton: &SkeletonSnapshot) -> Result<Self, RigError> {
        let index_of = |bone: RigBone| {
            skeleton
                .bones
                .iter()
                .position(|b| b.name == bone.name())
                .ok_or(RigError::MissingBone(bone))
        };
        let mut bones = [0; RigBone::COUNT];
        for bone in RigBone::ALL {
            bones[bone.index()] = index_of(bone)?;
        }
        for bone in RigBone::ALL {
            if let Some(parent) = bone.parent() {
                let actual = skeleton.bones[bones[bone.index()]].parent;
                if actual != Some(bones[parent.index()]) {
                    return Err(RigError::UnexpectedParent(bone));
                }
            }
        }
        let model = skeleton.reference_model_space();
        let reference = RigBone::ALL.map(|bone| model[bones[bone.index()]]);
        let mut arms = [ArmRig {
            shoulder_local: Vec3::ZERO,
            length: 0.0,
        }; 2];
        for side in Side::BOTH {
            let (clavicle, names) = side.arm_bones();
            let mut parent = bones[clavicle.index()];
            let mut chain = [0; 3];
            for (link, name) in chain.iter_mut().zip(names) {
                let index = skeleton
                    .bones
                    .iter()
                    .position(|b| b.name == name)
                    .ok_or(RigError::MissingArmBone(name))?;
                if skeleton.bones[index].parent != Some(parent) {
                    return Err(RigError::UnexpectedArmParent(name));
                }
                *link = index;
                parent = index;
            }
            let [upper, fore, hand] = chain.map(|i| model[i].translation);
            let clavicle = reference[clavicle.index()];
            arms[side.index()] = ArmRig {
                shoulder_local: clavicle.rotation.inverse() * (upper - clavicle.translation),
                length: upper.distance(fore) + fore.distance(hand),
            };
        }
        Ok(Self {
            bones,
            reference,
            arms,
        })
    }

    /// The skeleton index of `bone`.
    pub fn skeleton_index(&self, bone: RigBone) -> usize {
        self.bones[bone.index()]
    }

    /// The reference model-space transform of `bone`.
    pub fn reference(&self, bone: RigBone) -> &Transform {
        &self.reference[bone.index()]
    }

    /// The arm on `side`.
    pub fn arm(&self, side: Side) -> &ArmRig {
        &self.arms[side.index()]
    }
}

/// Why a skeleton cannot be rigged.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RigError {
    MissingBone(RigBone),
    UnexpectedParent(RigBone),
    /// An arm bone under a clavicle is missing.
    MissingArmBone(&'static str),
    /// An arm bone's parent is not the bone above it in the arm.
    UnexpectedArmParent(&'static str),
}

impl fmt::Display for RigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "procedural rig: ")?;
        match self {
            Self::MissingBone(bone) => write!(f, "the skeleton has no {} bone", bone.name()),
            Self::UnexpectedParent(bone) => write!(
                f,
                "{}'s parent is not {}",
                bone.name(),
                bone.parent().map_or("the root", RigBone::name)
            ),
            Self::MissingArmBone(name) => write!(f, "the skeleton has no {name} bone"),
            Self::UnexpectedArmParent(name) => {
                write!(f, "{name}'s parent is not the bone above it in the arm")
            }
        }
    }
}

impl std::error::Error for RigError {}

#[cfg(test)]
pub(crate) mod tests {
    use glam::{Quat, Vec3};

    use super::*;
    use crate::controllers::body::procedural::skeleton::Bone;

    /// A standing test skeleton shaped like the shipped one, with a root and an extra bone that
    /// the rig does not drive. Model space: +Y up, facing -Z, the left side at -X.
    pub(crate) fn skeleton() -> SkeletonSnapshot {
        let mut bones: Vec<Bone> = vec![];
        let add = |name: &str, parent: Option<&str>, model: Vec3, bones: &mut Vec<Bone>| {
            let parent_index = parent.map(|p| bones.iter().position(|b| b.name == p).unwrap());
            let parent_model = parent_index.map_or(Vec3::ZERO, |p| model_of(bones, p));
            bones.push(Bone {
                name: name.into(),
                parent: parent_index,
                reference: Transform {
                    translation: model - parent_model,
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            });
        };
        add("Reference", None, Vec3::ZERO, &mut bones);
        add(
            "Hips",
            Some("Reference"),
            Vec3::new(0.0, 1.0, 0.0),
            &mut bones,
        );
        add(
            "Pelvis",
            Some("Hips"),
            Vec3::new(0.0, 0.95, 0.0),
            &mut bones,
        );
        add(
            "LeftUpLeg",
            Some("Pelvis"),
            Vec3::new(-0.1, 0.92, 0.0),
            &mut bones,
        );
        add(
            "LeftLeg",
            Some("LeftUpLeg"),
            Vec3::new(-0.1, 0.5, -0.02),
            &mut bones,
        );
        add(
            "LeftFoot",
            Some("LeftLeg"),
            Vec3::new(-0.1, 0.08, 0.0),
            &mut bones,
        );
        add(
            "RightUpLeg",
            Some("Pelvis"),
            Vec3::new(0.1, 0.92, 0.0),
            &mut bones,
        );
        add(
            "RightLeg",
            Some("RightUpLeg"),
            Vec3::new(0.1, 0.5, -0.02),
            &mut bones,
        );
        add(
            "RightFoot",
            Some("RightLeg"),
            Vec3::new(0.1, 0.08, 0.0),
            &mut bones,
        );
        add("Spine", Some("Hips"), Vec3::new(0.0, 1.1, 0.0), &mut bones);
        add(
            "Spine1",
            Some("Spine"),
            Vec3::new(0.0, 1.2, 0.0),
            &mut bones,
        );
        add(
            "Spine2",
            Some("Spine1"),
            Vec3::new(0.0, 1.3, 0.0),
            &mut bones,
        );
        add(
            "Sternum",
            Some("Spine2"),
            Vec3::new(0.0, 1.4, 0.0),
            &mut bones,
        );
        add(
            "Neck",
            Some("Sternum"),
            Vec3::new(0.0, 1.55, 0.0),
            &mut bones,
        );
        add("Head", Some("Neck"), Vec3::new(0.0, 1.65, 0.0), &mut bones);
        add(
            "LeftShoulder",
            Some("Sternum"),
            Vec3::new(-0.05, 1.45, 0.0),
            &mut bones,
        );
        add(
            "RightShoulder",
            Some("Sternum"),
            Vec3::new(0.05, 1.45, 0.0),
            &mut bones,
        );
        for (side, x) in [("Left", -1.0), ("Right", 1.0)] {
            let chain = [
                (format!("{side}Arm"), format!("{side}Shoulder"), 0.2),
                (format!("{side}ForeArm"), format!("{side}Arm"), 0.5),
                (format!("{side}Hand"), format!("{side}ForeArm"), 0.78),
            ];
            for (name, parent, out) in chain {
                add(
                    &name,
                    Some(&parent),
                    Vec3::new(x * out, 1.45, 0.0),
                    &mut bones,
                );
            }
        }
        SkeletonSnapshot {
            name: "test".into(),
            bones,
        }
    }

    fn model_of(bones: &[Bone], index: usize) -> Vec3 {
        let mut position = Vec3::ZERO;
        let mut bone = Some(index);
        while let Some(i) = bone {
            position += bones[i].reference.translation;
            bone = bones[i].parent;
        }
        position
    }

    #[test]
    fn the_shipped_shape_rigs() {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        assert_eq!(rig.skeleton_index(RigBone::Hips), 1);
        assert!(
            rig.reference(RigBone::Head)
                .translation
                .abs_diff_eq(Vec3::new(0.0, 1.65, 0.0), 1e-6)
        );
    }

    #[test]
    fn a_missing_bone_is_reported() {
        let mut skeleton = skeleton();
        skeleton.bones.retain(|b| b.name != "Sternum");
        // Removing a bone shifts the parents after it, so only the name is checked here.
        assert_eq!(
            Rig::from_skeleton(&skeleton),
            Err(RigError::MissingBone(RigBone::Sternum))
        );
    }

    #[test]
    fn a_bone_in_between_is_refused() {
        let mut skeleton = skeleton();
        let spine = skeleton
            .bones
            .iter()
            .position(|b| b.name == "Spine")
            .unwrap();
        let spine1 = skeleton
            .bones
            .iter()
            .position(|b| b.name == "Spine1")
            .unwrap();
        skeleton.bones[spine1].parent = Some(spine - 1);
        assert_eq!(
            Rig::from_skeleton(&skeleton),
            Err(RigError::UnexpectedParent(RigBone::Spine1))
        );
    }

    #[test]
    fn the_arms_are_measured_from_the_reference_pose() {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        for side in Side::BOTH {
            let arm = rig.arm(side);
            assert!((arm.length - 0.58).abs() < 1e-5, "{arm:?}");
            let x = if side == Side::Left { -0.15 } else { 0.15 };
            assert!(arm.shoulder_local.abs_diff_eq(Vec3::new(x, 0.0, 0.0), 1e-5));
        }
        let mut skeleton = skeleton();
        for bone in &mut skeleton.bones {
            if bone.name == "RightForeArm" {
                bone.name = "RightForeArmRoll".into();
            }
        }
        assert_eq!(
            Rig::from_skeleton(&skeleton),
            Err(RigError::MissingArmBone("RightForeArm"))
        );
    }

    #[test]
    fn the_order_puts_parents_first() {
        for (i, bone) in RigBone::ALL.iter().enumerate() {
            assert_eq!(bone.index(), i);
            if let Some(parent) = bone.parent() {
                assert!(parent.index() < i);
            }
        }
        assert!(RigBone::LeftFoot.descends_from(RigBone::Hips));
        assert!(!RigBone::LeftFoot.descends_from(RigBone::Spine));
    }
}
