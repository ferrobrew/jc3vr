//! The body's version 8 events: Rico's pose before and after the solves with the HumanIK targets
//! queued for them, and the grounded rig's inputs, outputs, calibration, and tuning.

use glam::{Quat, Vec2, Vec3};

use crate::{event::Label, model::Pose};

/// The body bones a [`PoseSample`] of [`BoneSet::Body`] holds, in its order, by the skeleton's own
/// bone names.
pub const BODY_BONES: [&str; 23] = [
    "Hips",
    "Spine",
    "Spine1",
    "Spine2",
    "Sternum",
    "Neck",
    "Head",
    "LeftShoulder",
    "LeftArm",
    "LeftForeArm",
    "LeftHand",
    "RightShoulder",
    "RightArm",
    "RightForeArm",
    "RightHand",
    "LeftUpLeg",
    "LeftLeg",
    "LeftFoot",
    "LeftToeBase",
    "RightUpLeg",
    "RightLeg",
    "RightFoot",
    "RightToeBase",
];

/// The parent of each of [`BODY_BONES`] within the list, for drawing them as a skeleton; `None` for
/// the hips.
pub const BODY_BONE_PARENTS: [Option<usize>; 23] = [
    None,
    Some(0),
    Some(1),
    Some(2),
    Some(3),
    Some(4),
    Some(5),
    Some(4),
    Some(7),
    Some(8),
    Some(9),
    Some(4),
    Some(11),
    Some(12),
    Some(13),
    Some(0),
    Some(15),
    Some(16),
    Some(17),
    Some(0),
    Some(19),
    Some(20),
    Some(21),
];

/// Which bones a [`PoseSample`] lists. A new list is a new set, so the bones a recording holds can
/// grow without a format version.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BoneSet {
    /// [`BODY_BONES`].
    Body,
}

impl BoneSet {
    /// The bones' names, in the sample's order.
    pub fn names(self) -> &'static [&'static str] {
        match self {
            BoneSet::Body => &BODY_BONES,
        }
    }

    /// Each bone's parent within the set.
    pub fn parents(self) -> &'static [Option<usize>] {
        match self {
            BoneSet::Body => &BODY_BONE_PARENTS,
        }
    }
}

/// When in the pose-finalization pass a [`PoseSample`] was read.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PoseStage {
    /// The animated pose as the pose job left it, before the HumanIK solves: what a rig is layered
    /// on.
    PreIk,
    /// The pose after both HumanIK passes and every joint the mod writes after them: what renders.
    Final,
}

impl PoseStage {
    pub fn name(self) -> &'static str {
        match self {
            PoseStage::PreIk => "pre_ik",
            PoseStage::Final => "final",
        }
    }
}

/// The local player's pose at one stage of one pose-finalization pass, in the character's model
/// space, from version 8.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct PoseSample {
    /// The pose-finalization passes the recorder has seen since the payload loaded: the two stages
    /// of one pass share it.
    pub pass: u64,
    pub stage: PoseStage,
    /// The character's model-to-world transform the pass ran under: Rico's world position and
    /// heading.
    pub character: Pose,
    pub set: BoneSet,
    /// Each bone of the set in model space, `None` where the skeleton has no such bone or its
    /// position lies outside the encoding's range. Positions keep a tenth of a millimetre and
    /// rotations a hundredth of a degree.
    pub bones: Vec<Option<Pose>>,
    /// The HumanIK `MAIN`-pass targets live when the pass entered its solve, the game's and the
    /// mod's: recorded with the pre-IK stage.
    pub targets: Vec<IkTarget>,
}

/// One live HumanIK target.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct IkTarget {
    /// The HumanIK effector, as the solver numbers it.
    pub effector: i32,
    pub goal: IkGoal,
    /// The effector's reach weight for this kind of target, and its pull, `0..=1`.
    pub reach: f32,
    pub pull: f32,
}

/// What an [`IkTarget`] asks of its effector, in the character's model space.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum IkGoal {
    Position(Vec3),
    /// A rotation about `axis` by `angle_rad`, applied as an offset onto the pre-solve state.
    Rotation {
        axis: Vec3,
        angle_rad: f32,
    },
}

/// One grounded-rig frame: what the payload handed the rig and what it returned, from version 8.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct RigSample {
    pub input: RigInput,
    pub output: RigResult,
    /// The room height of the cockpit's floor: a room point is a cockpit point less this.
    pub floor_m: f32,
    /// Where a recenter put the root's cockpit point this frame, room `(x, z)`.
    pub snapped_to: Option<Vec2>,
}

/// The rig's inputs, in the room frame: metres over the floor, +Y up, -Z forward.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct RigInput {
    pub eyes: Vec3,
    pub head: Quat,
    /// The tracked grips, indexed by [`Hand::index`](crate::model::Hand::index).
    pub hands: [Option<Vec3>; 2],
    /// The floor point under Rico's pelvis, `(x, z)`.
    pub pelvis: Vec2,
    pub moving: bool,
    pub sample_height: bool,
    pub dt_s: f32,
}

/// The rig's outputs.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct RigResult {
    pub neck: Vec3,
    pub waist: Vec3,
    pub spine_m: f32,
    /// The neck's offset from the pelvis, `(x, z)`.
    pub offset: Vec2,
    /// The spine's bend, pitch and roll, radians.
    pub bend: Vec2,
    pub lean_reach_m: f32,
    pub step_demand: Vec2,
    pub crouch_m: f32,
    pub hands_reaching: bool,
    /// The step phase, as the rig names it: `idle`, `stepping`, `settling`, or `drifting`.
    pub phase: Label,
    pub gap: Vec2,
    pub body_yaw: f32,
    pub chest_yaw: f32,
    pub yaw_target: f32,
    pub turning: bool,
    pub turn_cause: Option<Label>,
    pub standing_neck_m: f32,
    pub steps: u64,
}

/// The rig's calibration and tuning, noted when either changes and repeated now and then, so that a
/// stretch of a recording opens with the ones in use, from version 8.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct RigConfig {
    /// The player's standing height and eye height the calibration holds, metres.
    pub height_m: f32,
    pub eye_height_m: f32,
    /// The rig's body: its standing eye height and waist pivot height, metres.
    pub body_eye_height_m: f32,
    pub body_hip_height_m: f32,
    /// The rig's tuning as JSON, in the form `rig-eval --tuning` reads.
    pub tuning_json: Label,
    /// Whether nothing changed since the last note, and this one only repeats it.
    pub repeat: bool,
}
