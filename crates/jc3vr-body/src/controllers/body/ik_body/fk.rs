//! The FK pelvis in the game: the pure follower ([`body_rig::fk`]) advanced once per rendered frame
//! from the root's frame half, in the rig's room frame, and published for the solve gate and the
//! root drive.
//!
//! The estimate's joint rotations are in the cockpit frame as it stands when read, and the room
//! frame is the cockpit frame with its floor at zero, so the rotations need no turn. The body is
//! published relative to the neck pivot, in room axes and the player's metres; the gate carries it
//! into Rico's model space and scales it to his proportions.

use std::time::Instant;

use body_estimator::smpl::JOINT_COUNT;
use body_rig::{
    PlayerBody,
    body::neck_pivot,
    fk::{BodyRotations, FkBody, FkFollower, FkFrame, FkSkeleton, FkTuning, PelvisSource},
};
use glam::{Quat, Vec2, Vec3};
use parking_lot::Mutex;

use crate::{controllers::body::estimator::Estimate, tuning::FRESHNESS};

/// One rendered frame's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct FkInput {
    /// The eyes, room frame.
    pub eyes: Vec3,
    /// The head's cockpit orientation.
    pub head: Quat,
    /// The latest estimate, whatever its age.
    pub estimate: Option<Estimate>,
    pub body: PlayerBody,
    /// The neck pivot's standing height, as the grounded rig measures it.
    pub standing_neck: f32,
    /// The grounded rig's waist pivot, room frame, while it runs.
    pub waist: Option<Vec3>,
    /// The heading the fallback faces: the grounded rig's legs'.
    pub yaw: f32,
    pub tuning: FkTuning,
    pub dt: f32,
    pub now: Instant,
}

/// The followed body as last published.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FkPose {
    /// The body's points relative to the neck pivot, room axes, the player's metres.
    pub body: FkBody,
    /// The neck pivot, room frame.
    pub neck: Vec3,
    pub skeleton: FkSkeleton,
    pub source: PelvisSource,
    pub estimate_weight: f32,
    pub at: Instant,
}

impl FkPose {
    /// The floor point under the pelvis, room `(x, z)`.
    pub fn pelvis_floor(&self) -> Vec2 {
        let pelvis = self.neck + self.body.pelvis;
        Vec2::new(pelvis.x, pelvis.z)
    }
}

/// Advance the follower by one frame and publish the body.
pub(crate) fn frame(input: &FkInput) -> FkPose {
    let neck = neck_pivot(input.eyes, input.head);
    let rotations = input
        .estimate
        .filter(|estimate| !estimate.stale())
        .map(|estimate| rotations(&estimate.joints));
    let skeleton = FkSkeleton::for_body(&input.body, input.standing_neck);
    let mut state = STATE.lock();
    let output = state.follower.update(
        &FkFrame {
            rotations,
            fallback_pelvis: input.waist.map(|waist| waist - neck),
            fallback_yaw: input.yaw,
            skeleton,
            dt: input.dt,
        },
        &input.tuning,
    );
    let pose = FkPose {
        body: output.body,
        neck,
        skeleton,
        source: output.source,
        estimate_weight: output.estimate_weight,
        at: input.now,
    };
    state.latest = Some(pose);
    pose
}

/// The latest published body, while it is fresh.
pub fn latest() -> Option<FkPose> {
    STATE.lock().latest.filter(|pose| {
        Instant::now().saturating_duration_since(pose.at)
            <= FRESHNESS.get().body.ik_fk_ms.duration()
    })
}

/// Forget the followed body, for a teleport: the next estimate is taken whole.
pub(crate) fn forget() {
    let mut state = STATE.lock();
    state.follower.forget();
    state.latest = None;
}

/// The FK's rotations from the estimate's joints, in SMPL's joint order
/// (`body_estimator::smpl::NAMES`).
fn rotations(joints: &[Quat; JOINT_COUNT]) -> BodyRotations {
    BodyRotations {
        pelvis: joints[PELVIS],
        spine1: joints[SPINE1],
        spine2: joints[SPINE2],
        spine3: joints[SPINE3],
        neck: joints[NECK],
        hips: [joints[LEFT_HIP], joints[RIGHT_HIP]],
        knees: [joints[LEFT_KNEE], joints[RIGHT_KNEE]],
    }
}

/// The SMPL joints the FK reads, by their place in the format's joint order.
const PELVIS: usize = body_estimator::smpl::PELVIS;
const LEFT_HIP: usize = 1;
const RIGHT_HIP: usize = 2;
const SPINE1: usize = 3;
const LEFT_KNEE: usize = 4;
const RIGHT_KNEE: usize = 5;
const SPINE2: usize = 6;
const SPINE3: usize = body_estimator::smpl::SPINE3;
const NECK: usize = 12;

struct State {
    follower: FkFollower,
    latest: Option<FkPose>,
}

static STATE: Mutex<State> = Mutex::new(State {
    follower: FkFollower::new(),
    latest: None,
});

#[cfg(test)]
mod tests {
    use body_estimator::smpl::NAMES;

    use super::*;

    #[test]
    fn the_joints_are_read_by_their_smpl_names() {
        for (index, name) in [
            (PELVIS, "pelvis"),
            (LEFT_HIP, "left_hip"),
            (RIGHT_HIP, "right_hip"),
            (SPINE1, "spine1"),
            (LEFT_KNEE, "left_knee"),
            (RIGHT_KNEE, "right_knee"),
            (SPINE2, "spine2"),
            (SPINE3, "spine3"),
            (NECK, "neck"),
        ] {
            assert_eq!(NAMES[index], name);
        }
    }

    #[test]
    fn the_pelvis_floor_point_is_under_the_published_pelvis() {
        let body = PlayerBody::from_heights(1.8, 1.68);
        let skeleton = FkSkeleton::for_body(&body, body.standing_neck());
        let fk = body_rig::fk::fallback(Vec3::new(0.1, -0.6, 0.2), 0.0, &skeleton);
        let pose = FkPose {
            body: fk,
            neck: Vec3::new(1.0, 1.6, -2.0),
            skeleton,
            source: PelvisSource::Fallback,
            estimate_weight: 0.0,
            at: Instant::now(),
        };
        assert!((pose.pelvis_floor() - Vec2::new(1.1, -1.8)).length() < 1e-6);
    }
}
