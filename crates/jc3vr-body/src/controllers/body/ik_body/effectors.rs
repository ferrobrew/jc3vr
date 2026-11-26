//! The IK body's HumanIK targets: the hips held on the seeded pelvis, the ankles and toes on the
//! planted contacts or the air pose, and the knees hinted, each owned outright against the game's
//! locomotion IK task at full weight and handed back to it as the weight falls.
//!
//! The locomotion IK task queues targets on the hips and the feet every on-foot tick before the
//! solve gate ([`NLocoIKTask_Update`](jc3gi::animation::arm_aim_ik::NLocoIKTask_Update)). A pass
//! holds one target per effector and a later queue overwrites it in place
//! ([`HumanIK::AddEffectorTargetPosition`]), so each target here replaces the game's, at the game's
//! own target mixed toward the IK's by the weight, with the reach, pull, and resistance mixed the
//! same way. The targets are queued without the blend-out flag, so on the first tick the IK stops
//! queuing them they are dropped rather than left steering while their reach decays.
//!
//! The hips' orientation is held with a zero rotation offset: the solver applies a rotation target
//! as an offset onto the pose as it stands before the solve
//! ([`HumanIK::UpdateEffectorsFromTargets`]), which is the seeded pose, so a zero offset with full
//! reach and resistance keeps the seeded pelvis's orientation through the solve.

use body_rig::math::angle::forward;
use glam::Vec3;
use jc3gi::{
    animation::ik::{HumanIK, Pass, SolveStep},
    types::math::Vector3,
};

use crate::controllers::body::{
    ik_body::{
        STATE,
        tuning::{Gains, IK_BODY},
    },
    procedural::{
        rig::{Rig, RigBone},
        solver::pose::RigPose,
    },
};

/// Queue the targets [`prepare`](crate::controllers::body::ik_body::gate::prepare) left for this
/// tick on the `MAIN` pass of `hik`, for the written `pose`. Returns how many it queued.
///
/// # Safety
///
/// `hik` must be the live local character's solver, inside its pose-finalization pass before the
/// `MAIN` solve, on the thread running it; `rig` must be the rig of its skeleton and `pose` the
/// pose written this tick.
pub(crate) unsafe fn queue(hik: &mut HumanIK, rig: &Rig, pose: &RigPose) -> usize {
    let Some(targets) = STATE.lock().targets.take() else {
        return 0;
    };
    let w = targets.weights;
    let gains = &IK_BODY.get().effectors;
    let mut queued = 0;
    let mut writer = Writer { hik, rig, pose };
    if w.pelvis_position > 0.0 {
        let hips = pose.position(RigBone::Hips);
        queued += usize::from(unsafe {
            writer.place(RigBone::Hips, hips, w.pelvis_position, gains.hips)
        });
        queued += usize::from(unsafe {
            writer.hold_rotation(
                RigBone::Hips,
                w.pelvis_orientation,
                gains.hips_rotation_reach,
                gains.hips_resist,
            )
        });
    }
    let feet = [
        (RigBone::LeftFoot, RigBone::LeftToeBase, RigBone::LeftLeg),
        (RigBone::RightFoot, RigBone::RightToeBase, RigBone::RightLeg),
    ];
    for (i, (ankle, toe, knee)) in feet.into_iter().enumerate() {
        let foot = gains.foot;
        let hint = gains.knee_hint;
        if w.feet > 0.0
            && let Some((brace, share)) = targets.braces[i]
        {
            // A braced foot mixes from the hanging leg's air pose toward its brace by the brace's
            // share, so a foot taking or leaving the face moves rather than jumps.
            let air = targets.air_ankles.map(|ankles| ankles[i]);
            let ankle_at = air.map_or(brace.ankle, |a| a.lerp(brace.ankle, share));
            let knee_at = targets.knees[i].lerp(brace.knee, share);
            let weight = if air.is_some() {
                w.feet.max(w.air_legs)
            } else {
                w.feet * share
            };
            queued += usize::from(unsafe { writer.place(ankle, ankle_at, weight, foot) });
            queued += usize::from(unsafe { writer.place(toe, brace.toe, w.feet * share, foot) });
            queued += usize::from(unsafe { writer.place(knee, knee_at, weight, hint) });
        } else if w.feet > 0.0
            && let Some(target) = targets.feet[i]
        {
            let ground = target.ankle.y - targets.ankle_height_m;
            let ahead = forward(target.yaw) * targets.toe_ahead_m;
            let toe_at = Vec3::new(
                target.ankle.x + ahead.x,
                ground + targets.toe_height_m,
                target.ankle.z + ahead.y,
            );
            queued += usize::from(unsafe { writer.place(ankle, target.ankle, w.feet, foot) });
            queued += usize::from(unsafe { writer.place(toe, toe_at, w.feet, foot) });
            queued += usize::from(unsafe { writer.place(knee, targets.knees[i], w.feet, hint) });
        } else if w.air_legs > 0.0
            && let Some(ankles) = targets.air_ankles
        {
            let air = gains.air_foot;
            queued += usize::from(unsafe { writer.place(ankle, ankles[i], w.air_legs, air) });
            queued +=
                usize::from(unsafe { writer.place(knee, targets.knees[i], w.air_legs, hint) });
        }
    }
    let mut state = STATE.lock();
    state.status.targets_queued = queued;
    queued
}

/// The solver and the pose the targets are queued against.
struct Writer<'a> {
    hik: &'a mut HumanIK,
    rig: &'a Rig,
    pose: &'a RigPose,
}

impl Writer<'_> {
    /// The effector driving `bone`, if the characterization maps one.
    unsafe fn effector(&self, bone: RigBone) -> Option<i32> {
        let index = u32::try_from(self.rig.skeleton_index(bone)).ok()?;
        let effector = unsafe { self.hik.GetEffectorIdFromBoneIndex(index) };
        (effector >= 0 && (effector as u64) < HumanIK::EFFECTOR_SLOTS).then_some(effector)
    }

    /// Queue a position target on `bone`'s effector at the game's own target (or the bone where
    /// the game queued none) mixed toward `ours` by `weight`, with the reach and pull mixed from
    /// the game's toward `gains`. Returns whether it queued one.
    unsafe fn place(&mut self, bone: RigBone, ours: Vec3, weight: f32, gains: Gains) -> bool {
        let weight = weight.clamp(0.0, 1.0);
        if weight <= 0.0 || !ours.is_finite() {
            return false;
        }
        let Some(effector) = (unsafe { self.effector(bone) }) else {
            return false;
        };
        let slot = effector as usize;
        let game = unsafe { live_position(self.hik, effector) };
        let from = game.unwrap_or(self.pose.position(bone));
        let at = from.lerp(ours, weight);
        let target = Vector3 {
            data: [at.x, at.y, at.z],
        };
        if !target.data.iter().all(|v| v.is_finite()) {
            return false;
        }
        let (game_reach, game_pull) = match game {
            Some(_) => (self.hik.m_TargetReachT[slot], self.hik.m_TargetPull[slot]),
            None => (0.0, 0.0),
        };
        unsafe {
            self.hik.AddEffectorTargetPosition(
                effector,
                &target,
                SolveStep::SPINE_HEAD_LOWER_BODY,
                Pass::MAIN,
                false,
                INTERPOLATION_RATE,
                false,
                BLEND_OUT_RATE,
            );
        }
        self.hik.m_TargetReachT[slot] = lerp(game_reach, gains.reach, weight);
        self.hik.m_TargetPull[slot] = lerp(game_pull, gains.pull, weight);
        true
    }

    /// Queue a zero rotation offset on `bone`'s effector, holding it as the seeded pose stands, in
    /// place of the game's offset scaled by what is left of `weight`, with the rotation reach and
    /// the resistance mixed from the game's toward `reach` and `resist`. Returns whether it queued
    /// one.
    unsafe fn hold_rotation(
        &mut self,
        bone: RigBone,
        weight: f32,
        reach: f32,
        resist: f32,
    ) -> bool {
        let weight = weight.clamp(0.0, 1.0);
        if weight <= 0.0 {
            return false;
        }
        let Some(effector) = (unsafe { self.effector(bone) }) else {
            return false;
        };
        let slot = effector as usize;
        let game = unsafe { live_rotation(self.hik, effector) };
        let (axis, angle) = game.unwrap_or((Vec3::Y, 0.0));
        let axis = Vector3 {
            data: [axis.x, axis.y, axis.z],
        };
        let angle = angle * (1.0 - weight);
        if !angle.is_finite() || !axis.data.iter().all(|v| v.is_finite()) {
            return false;
        }
        let (game_reach, game_resist) = match game {
            Some(_) => (self.hik.m_TargetReachR[slot], self.hik.m_TargetResist[slot]),
            None => (0.0, 0.0),
        };
        unsafe {
            self.hik.AddEffectorTargetRotationVector(
                effector,
                angle,
                &axis,
                SolveStep::SPINE_HEAD_LOWER_BODY,
                Pass::MAIN,
                false,
                INTERPOLATION_RATE,
                false,
                BLEND_OUT_RATE,
            );
        }
        self.hik.m_TargetReachR[slot] = lerp(game_reach, reach, weight);
        self.hik.m_TargetResist[slot] = lerp(game_resist, resist, weight);
        true
    }
}

/// The interpolation and blend-out rates the targets carry; both flags are off, so the rates only
/// fill the entry as the game's own foot targets fill it.
const INTERPOLATION_RATE: f32 = 3.0;
const BLEND_OUT_RATE: f32 = 1.5;

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    let from = if from.is_finite() { from } else { 0.0 };
    from + (to - from) * t
}

/// The position of the live `MAIN` position target on `effector`, model space, when the pass has
/// one queued this tick.
///
/// # Safety
///
/// As [`queue`].
unsafe fn live_position(hik: &HumanIK, effector: i32) -> Option<Vec3> {
    let positions = &hik.m_PassInfo[Pass::MAIN as usize].m_EffectorTargetPositions;
    unsafe { positions.as_slice() }
        .iter()
        .find(|entry| entry.effector == effector && entry.is_valid)
        .map(|entry| Vec3::from_array(entry.effector_position.data))
}

/// The axis and angle of the live `MAIN` rotation target on `effector`, when the pass has one
/// queued this tick.
///
/// # Safety
///
/// As [`queue`].
unsafe fn live_rotation(hik: &HumanIK, effector: i32) -> Option<(Vec3, f32)> {
    let rotations = &hik.m_PassInfo[Pass::MAIN as usize].m_EffectorTargetRotations;
    unsafe { rotations.as_slice() }
        .iter()
        .find(|entry| entry.effector == effector && entry.is_valid)
        .map(|entry| {
            (
                Vec3::from_array(entry.effector_rotation_axis.data),
                entry.effector_rotation_angle,
            )
        })
        .and_then(|(axis, angle)| Some((axis.try_normalize()?, angle)))
        .filter(|(_, angle)| angle.is_finite())
}
