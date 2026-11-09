//! The skeleton bones the palette edit touches, and the post-solve arm joints each tick publishes
//! for the edit and the status readout.

use glam::{Mat4, Quat, Vec3};
use jc3gi::character::character::AnimationController;
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    hands::{ik, palette},
};

/// The largest skeleton bone index accepted from a name lookup; the engine's bone indices are
/// `i16`-sized.
pub const MAX_BONE_INDEX: u32 = 0x7FFF;

/// The bones the palette edit touches for one hand: the hand group (the wrist, the attach bones,
/// and the fingers), which takes the hand's whole correction, and the forearm group, which swings
/// about the elbow. Zero marks a bone that did not resolve.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandBones {
    pub(super) hand: [u16; HAND_BONE_NAMES[0].len()],
    pub(super) forearm: [u16; FOREARM_BONE_NAMES[0].len()],
    pub(super) bind: [Mat4; 2],
    pub(super) roll: [f32; FOREARM_BONE_NAMES[0].len()],
    pub(super) roll_bind: [Quat; FOREARM_BONE_NAMES[0].len()],
}

impl Default for HandBones {
    fn default() -> Self {
        Self {
            hand: [0; 20],
            forearm: [0; 4],
            bind: [Mat4::IDENTITY; 2],
            roll: [0.0; 4],
            roll_bind: [Quat::IDENTITY; 4],
        }
    }
}

impl HandBones {
    /// Resolve `hand`'s bones by name.
    ///
    /// # Safety
    ///
    /// `controller` must be the character's live animation controller, on the thread that owns it.
    pub(crate) unsafe fn resolve(controller: &AnimationController, hand: Hand) -> Self {
        let resolve = |name| unsafe { ik::bone_index(controller, name) }.map_or(0, |i| i as u16);
        let side = hand.index();
        let hand = HAND_BONE_NAMES[side].map(resolve);
        let forearm = FOREARM_BONE_NAMES[side].map(resolve);
        let matrix = |bone: u16| {
            let mut matrix = jc3gi::types::math::Matrix4::from(Mat4::IDENTITY);
            if bone != 0 {
                unsafe { controller.GetBindPoseBoneMatrix(bone as u32, &mut matrix) };
            }
            Mat4::from(matrix)
        };
        let bind = [matrix(forearm[0]), matrix(hand[0])];
        let elbow = bind[0].w_axis.truncate();
        let axis = bind[1].w_axis.truncate() - elbow;
        let roll = forearm.map(|bone| {
            if bone == 0 || axis.length_squared() <= 1e-8 {
                return 0.0;
            }
            ((matrix(bone).w_axis.truncate() - elbow).dot(axis) / axis.length_squared())
                .clamp(0.0, 1.0)
        });
        HandBones {
            hand,
            forearm,
            bind,
            roll,
            roll_bind: forearm
                .map(|bone| palette::bone_rotation(matrix(bone)).unwrap_or(Quat::IDENTITY)),
        }
    }

    /// The wrist bone (the hand group's root).
    pub fn wrist(&self) -> u16 {
        self.hand[0]
    }

    /// The elbow's bone (the forearm, whose origin is the elbow).
    pub fn elbow(&self) -> u16 {
        self.forearm[0]
    }

    pub(super) fn hand_bones(&self) -> impl Iterator<Item = usize> + '_ {
        self.hand.iter().filter(|&&b| b != 0).map(|&b| b as usize)
    }

    pub(super) fn forearm_bones(&self) -> impl Iterator<Item = usize> + '_ {
        self.forearm
            .iter()
            .filter(|&&b| b != 0)
            .map(|&b| b as usize)
    }
}

/// The post-solve model-space elbow and wrist of one arm, and the solved wrist's shortfall from the
/// tick's target.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmJoints {
    pub elbow: Vec3,
    pub wrist: Vec3,
    /// The tick's world-space wrist target less the solved wrist in the world; `None` when the
    /// hand had no target this tick.
    pub miss: Option<Vec3>,
}

/// The published bones and post-solve arm joints of the last two ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct Solved {
    pub(super) bones: [HandBones; 2],
    pub(super) previous: [Option<ArmJoints>; 2],
    pub(super) current: [Option<ArmJoints>; 2],
}

/// Publish a tick's resolved bones and post-solve arm joints; the previous tick's joints shift
/// back.
pub(crate) fn publish_solved(bones: [HandBones; 2], joints: [Option<ArmJoints>; 2]) {
    let mut solved = SOLVED.lock();
    solved.bones = bones;
    solved.previous = solved.current;
    solved.current = joints;
}

/// The frame snapshot's read of the last two ticks' published solve.
pub(super) fn solved() -> Solved {
    *SOLVED.lock()
}

/// The hand group per side: the wrist first, then the attach bones and the seventeen finger bones.
const HAND_BONE_NAMES: [[&str; 20]; 2] = [
    [
        "LeftHand",
        "LeftHandAttach",
        "LeftHandAttach2",
        "LeftHandThumb1",
        "LeftHandThumb2",
        "LeftHandThumb3",
        "LeftHandIndex1",
        "LeftHandIndex2",
        "LeftHandIndex3",
        "LeftHandMiddle1",
        "LeftHandMiddle2",
        "LeftHandMiddle3",
        "LeftInHandRing",
        "LeftHandRing1",
        "LeftHandRing2",
        "LeftHandRing3",
        "LeftInHandPinky",
        "LeftHandPinky1",
        "LeftHandPinky2",
        "LeftHandPinky3",
    ],
    [
        "RightHand",
        "RightHandAttach",
        "RightHandAttach2",
        "RightHandThumb1",
        "RightHandThumb2",
        "RightHandThumb3",
        "RightHandIndex1",
        "RightHandIndex2",
        "RightHandIndex3",
        "RightHandMiddle1",
        "RightHandMiddle2",
        "RightHandMiddle3",
        "RightInHandRing",
        "RightHandRing1",
        "RightHandRing2",
        "RightHandRing3",
        "RightInHandPinky",
        "RightHandPinky1",
        "RightHandPinky2",
        "RightHandPinky3",
    ],
];

/// The forearm group per side: the forearm (whose origin is the elbow) first, then its roll bones.
const FOREARM_BONE_NAMES: [[&str; 4]; 2] = [
    [
        "LeftForeArm",
        "LeftForeArmRoll",
        "df_LeftForeArmRollB01",
        "df_LeftForeArmRollC01",
    ],
    [
        "RightForeArm",
        "RightForeArmRoll",
        "df_RightForeArmRollB01",
        "df_RightForeArmRollC01",
    ],
];

static SOLVED: Mutex<Solved> = Mutex::new(Solved {
    bones: [HandBones {
        hand: [0; 20],
        forearm: [0; 4],
        bind: [Mat4::IDENTITY; 2],
        roll: [0.0; 4],
        roll_bind: [Quat::IDENTITY; 4],
    }; 2],
    previous: [None; 2],
    current: [None; 2],
});
