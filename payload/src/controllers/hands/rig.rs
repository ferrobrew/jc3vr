//! The character's arms measured from his animated skeleton: each arm's upper-arm and forearm
//! lengths and the shoulder joint's offset from the eye midpoint, read at the pre-solve seam every
//! tick and smoothed ([`crate::controllers::hands::reach::ArmTracker`]).
//!
//! The joints are the bones HumanIK drives for the shoulder, elbow, and wrist effectors, found by
//! asking the solver ([`body_ik::bone_for_effector`]) rather than by name, so the measured arm is
//! exactly the chain the wrist targets are solved along. The eye midpoint is the midpoint of the
//! eye bones, the point the view is anchored at. All are read before the solve, so they are the
//! animated pose, never the previous tick's IK.

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    animation::ik::Effector,
    character::character::{AnimationController, Character},
};
use parking_lot::Mutex;

use crate::{
    controllers::{
        body::procedural::skeleton::SkeletonKey,
        frame::Hand,
        hands::reach::{ArmSample, ArmTracker, MeasuredArm},
    },
    headpose,
    hooks::character::{
        body_ik,
        pose::{self, LivePose},
    },
};

/// The measured arms, indexed by [`Hand::index`]; `None` for an arm not yet measured.
///
/// On the game thread the measurements are first revalidated against the live local character
/// ([`prepare`]). Elsewhere, including the input job (which overlaps the character manager's
/// pre-simulation step even when the game thread runs it), the retained measurements are composed
/// as they are; the input tick's callers run in the game stage, which revalidates every tick.
pub fn arms() -> [Option<MeasuredArm>; 2] {
    if matches!(
        crate::thread_role::current(),
        crate::thread_role::ThreadRole::Game
            | crate::thread_role::ThreadRole::GameFrame
            | crate::thread_role::ThreadRole::GameDebugUi
    ) {
        unsafe {
            match Character::GetLocalPlayerCharacter().as_ref() {
                Some(character) => prepare(character),
                None => invalidate(),
            }
        }
    }
    STATE
        .lock()
        .arms_with(Instant::now(), headpose::xr::body_rotation)
}

/// Check the current native identity before composing any hand targets from retained measurements.
///
/// # Safety
///
/// The local character and controller must be live on their serialized owning thread.
pub(crate) unsafe fn prepare(character: &Character) {
    let identity = (|| {
        if !pose::eligible(character) {
            return None;
        }
        let controller = unsafe { character.m_AnimatedModel.m_AnimationController.as_ref() }?;
        unsafe { pose::identity(controller) }
    })();
    let mut state = STATE.lock();
    match identity {
        Some(key) => {
            let (_, rotation, _) =
                Mat4::from(character.m_WorldMatrixT1).to_scale_rotation_translation();
            state.prepare(key, rotation);
        }
        None => state.invalidate(),
    }
}

pub(crate) fn identity() -> Option<(SkeletonKey, usize)> {
    STATE.lock().key
}

pub(crate) fn invalidate() {
    STATE.lock().invalidate();
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
    if !pose::eligible(character) {
        STATE.lock().invalidate();
        return [None; 2];
    }
    let Some(pose) = (unsafe { LivePose::new(controller) }) else {
        STATE.lock().invalidate();
        return [None; 2];
    };
    let point = |index| {
        pose.read(index)
            .map(|joint| Vec3::from_array(joint.m_Translation.data))
    };
    let eyes = pose
        .named(b"fLeftEye")
        .and_then(point)
        .zip(pose.named(b"fRightEye").and_then(point))
        .map(|(left, right)| (left + right) * 0.5);
    let (_, character_rotation, _) =
        Mat4::from(character.m_WorldMatrixT1).to_scale_rotation_translation();
    let to_body = Quat::IDENTITY;

    let now = Instant::now();
    let mut state = STATE.lock();
    let key = (pose.key(), pose.producer());
    state.prepare(key, character_rotation);
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
        let joints = (
            bone(shoulder).and_then(point),
            bone(elbow).and_then(point),
            bone(wrist).and_then(point),
            eyes,
        );
        let (Some(shoulder), Some(elbow), Some(wrist), Some(eyes)) = joints else {
            state.resolved[hand.index()] = false;
            return None;
        };
        let sample = ArmSample {
            shoulder,
            elbow,
            wrist,
            eyes,
            to_body,
        };
        state.resolved[hand.index()] = sample.plausible();
        let tracker = &mut state.trackers[hand.index()];
        tracker.update(sample, dt, shoulder_smoothing_s);
        let arm = tracker.arm().filter(|_| sample.plausible());
        arm.map(|arm| TickArm {
            shoulder_model: sample.shoulder,
            length: arm.length(),
        })
    })
}

struct State {
    trackers: [ArmTracker; 2],
    resolved: [bool; 2],
    last: Option<Instant>,
    key: Option<(SkeletonKey, usize)>,
    character_rotation: Quat,
}

impl State {
    const EMPTY: Self = Self {
        trackers: [ArmTracker::EMPTY; 2],
        resolved: [false; 2],
        last: None,
        key: None,
        character_rotation: Quat::IDENTITY,
    };

    fn invalidate(&mut self) {
        *self = Self::EMPTY;
    }

    fn prepare(&mut self, key: (SkeletonKey, usize), character_rotation: Quat) {
        if self.key != Some(key) {
            self.invalidate();
        }
        self.key = Some(key);
        self.character_rotation = character_rotation;
    }

    #[cfg(test)]
    fn arms(&self, now: Instant, view: Quat) -> [Option<MeasuredArm>; 2] {
        self.arms_with(now, || view)
    }

    fn arms_with(&self, now: Instant, view: impl FnOnce() -> Quat) -> [Option<MeasuredArm>; 2] {
        let fresh = self.key.is_some()
            && self.last.is_some_and(|last| {
                now >= last && now.duration_since(last) <= Duration::from_millis(250)
            });
        if !fresh || !self.resolved.iter().any(|resolved| *resolved) {
            return [None; 2];
        }
        let to_view = view().inverse() * self.character_rotation;
        Hand::BOTH.map(|hand| {
            if !self.resolved[hand.index()] {
                return None;
            }
            self.trackers[hand.index()].arm().map(|mut arm| {
                arm.eye_to_shoulder = to_view * arm.eye_to_shoulder;
                arm
            })
        })
    }
}

static STATE: Mutex<State> = Mutex::new(State::EMPTY);

#[cfg(test)]
pub(crate) fn test_arms(view: Quat) -> [Option<MeasuredArm>; 2] {
    STATE.lock().arms(Instant::now(), view)
}

#[cfg(test)]
pub(crate) fn seed_test_measurement() {
    let mut state = STATE.lock();
    state.last = Some(Instant::now());
    state.resolved = [true; 2];
    for tracker in &mut state.trackers {
        tracker.update(
            ArmSample {
                shoulder: Vec3::new(0.2, 1.4, 0.0),
                elbow: Vec3::new(0.5, 1.4, 0.0),
                wrist: Vec3::new(0.8, 1.4, 0.0),
                eyes: Vec3::new(0.0, 1.7, 0.0),
                to_body: Quat::IDENTITY,
            },
            0.0,
            0.1,
        );
    }
}

#[cfg(test)]
mod tests;
