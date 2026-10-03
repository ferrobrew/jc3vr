//! The character's arms measured from his animated skeleton: each arm's upper-arm and forearm
//! lengths and the shoulder joint's offset from the eye midpoint, read at the pre-solve seam every
//! tick and smoothed ([`crate::controllers::hands::reach::ArmTracker`]).
//!
//! The joints are the bones HumanIK drives for the shoulder, elbow, and wrist effectors, found by
//! asking the solver ([`body_ik::bone_for_effector`]) rather than by name, so the measured arm is
//! exactly the chain the wrist targets are solved along. The eye midpoint is the midpoint of the
//! eye bones, the point the view is anchored at. All are read before the solve, so they are the
//! animated pose, never the previous tick's IK.

use std::time::Instant;

use glam::{Mat4, Vec3};
use jc3gi::{
    animation::ik::Effector,
    character::character::{AnimationController, Character, Joint},
};
use parking_lot::Mutex;

use crate::{
    controllers::{
        frame::Hand,
        hands::{
            ik,
            reach::{ArmSample, ArmTracker, MeasuredArm},
        },
    },
    headpose,
    hooks::character::body_ik,
};

/// The measured arms, indexed by [`Hand::index`]; `None` for an arm not yet measured.
pub fn arms() -> [Option<MeasuredArm>; 2] {
    STATE.lock().trackers.map(|t| t.arm())
}

/// One arm's measurement state, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ArmStatus {
    pub arm: Option<MeasuredArm>,
    pub samples: u32,
    pub rejected: u32,
    /// Whether the joints resolved on the last tick.
    pub resolved: bool,
}

/// The measurement state of both arms, indexed by [`Hand::index`].
pub fn status() -> [ArmStatus; 2] {
    let state = STATE.lock();
    Hand::BOTH.map(|hand| {
        let tracker = &state.trackers[hand.index()];
        ArmStatus {
            arm: tracker.arm(),
            samples: tracker.samples(),
            rejected: tracker.rejected(),
            resolved: state.resolved[hand.index()],
        }
    })
}

/// One arm as the pre-solve seam read it this tick: the shoulder joint in model space and the
/// smoothed arm length, for the pull ramp.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TickArm {
    pub shoulder_model: Vec3,
    pub length: f32,
}

/// Read both arms and the eye midpoint from the pre-solve pose and fold them into the measurement.
/// Returns this tick's reading of each arm, `None` for an arm whose joints did not resolve or that
/// has no measurement yet.
///
/// # Safety
///
/// `character` must be the live local character and `controller` its animation controller, on the
/// thread running its pose-finalization pass.
pub(crate) unsafe fn measure(
    character: &Character,
    controller: &mut AnimationController,
    shoulder_smoothing_s: f32,
) -> [Option<TickArm>; 2] {
    let eyes = unsafe {
        match (
            ik::bone_index(controller, "fLeftEye"),
            ik::bone_index(controller, "fRightEye"),
        ) {
            (Some(left), Some(right)) => {
                Some((joint_position(controller, left) + joint_position(controller, right)) * 0.5)
            }
            _ => None,
        }
    };
    let (_, character_rotation, _) =
        Mat4::from(character.m_WorldMatrixT1).to_scale_rotation_translation();
    let to_body = (headpose::xr::body_rotation().inverse() * character_rotation).normalize();

    let now = Instant::now();
    let mut state = STATE.lock();
    let dt = state
        .last
        .map_or(0.0, |last| now.duration_since(last).as_secs_f32());
    state.last = Some(now);

    Hand::BOTH.map(|hand| {
        let (shoulder, elbow, wrist) = match hand {
            Hand::Left => (
                Effector::LEFT_SHOULDER,
                Effector::LEFT_ELBOW,
                Effector::LEFT_WRIST,
            ),
            Hand::Right => (
                Effector::RIGHT_SHOULDER,
                Effector::RIGHT_ELBOW,
                Effector::RIGHT_WRIST,
            ),
        };
        let bone = |effector: Effector| unsafe {
            body_ik::bone_for_effector(&character.m_HIK, effector as i32)
        };
        let joints = (bone(shoulder), bone(elbow), bone(wrist), eyes);
        let (Some(shoulder), Some(elbow), Some(wrist), Some(eyes)) = joints else {
            state.resolved[hand.index()] = false;
            return None;
        };
        state.resolved[hand.index()] = true;
        let sample = unsafe {
            ArmSample {
                shoulder: joint_position(controller, shoulder),
                elbow: joint_position(controller, elbow),
                wrist: joint_position(controller, wrist),
                eyes,
                to_body,
            }
        };
        let tracker = &mut state.trackers[hand.index()];
        tracker.update(sample, dt, shoulder_smoothing_s);
        let arm = tracker.arm().filter(|_| sample.plausible());
        arm.map(|arm| TickArm {
            shoulder_model: sample.shoulder,
            length: arm.length(),
        })
    })
}

unsafe fn joint_position(controller: &mut AnimationController, bone: u32) -> Vec3 {
    let mut joint = Joint::default();
    unsafe { controller.GetJoint(bone, &mut joint) };
    Vec3::from_array(joint.m_Translation.data)
}

struct State {
    trackers: [ArmTracker; 2],
    resolved: [bool; 2],
    last: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    trackers: [ArmTracker::EMPTY; 2],
    resolved: [false; 2],
    last: None,
});
