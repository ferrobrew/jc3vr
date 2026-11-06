//! Rico's feet squared under him while he stands idle on foot (`docs/mod/body/procedural-body.md`,
//! "The stance").
//!
//! Every idle clip in Rico's set stands bladed, one foot ahead on a line turned well off his
//! facing, and the yaw follow squares only the hips ([`crate::controllers::body::procedural::hips`]).
//! The game's locomotion IK task already queues a position target on each ankle and each toe every
//! on-foot tick, at the clip's own feet raised or lowered to the ground under them
//! ([`NLocoIKTask_Update`](jc3gi::animation::arm_aim_ik::NLocoIKTask_Update)). At the solve gate,
//! while Rico stands still, those targets are moved across the ground to a square stance under the
//! squared hips, keeping each target's height, so the ground's adjustment stays the game's. Where
//! the game queued none, the gate queues its own at the clip's height. The move blends in once Rico
//! stands still and out as soon as he moves, so the walk's first step and the walk's stop start
//! from the clip's feet.

use body_rig::{
    angle::{forward, right, wrap},
    smooth::follow_factor,
};
use glam::{Vec2, Vec3};
use jc3gi::{
    animation::ik::{HumanIK, Pass, SolveStep},
    types::math::Vector3,
};
use serde::{Deserialize, Serialize};

use crate::controllers::body::procedural::{
    rig::{Rig, RigBone},
    solver::{arms::Side, pose::RigPose},
};

/// The stance's settings, `controllers.body.procedural.stance`.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StanceConfig {
    /// Square Rico's feet while he stands idle on foot.
    pub enabled: bool,
    /// How far apart the ankles stand, metres.
    pub width_m: f32,
    /// How far each foot turns out from the facing, degrees.
    pub toe_out_deg: f32,
    /// How long the stance takes to blend in once Rico stands still, and out once he moves,
    /// seconds: the time constant of its follower.
    pub blend_s: f32,
}

impl StanceConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            width_m: 0.29,
            toe_out_deg: 10.0,
            blend_s: 0.25,
        }
    }
}

impl Default for StanceConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// One foot's targets across the ground, model `(x, z)`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FootPlacement {
    pub ankle: Vec2,
    pub toe: Vec2,
}

/// The square stance for the animated pose `animated`, facing the model-space heading `heading`
/// (the clip's hips' heading, which the yaw follow turns to face the player): the ankles
/// `width_m` apart square across the heading, centred across it under the hips and along it where
/// the clip's ankles stand on average, and each toe ahead of its ankle by the clip's own foot
/// length, turned out by `toe_out_deg`. Left, then right.
pub fn square_stance(
    animated: &RigPose,
    heading: f32,
    config: &StanceConfig,
) -> [FootPlacement; 2] {
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    let ahead = forward(heading);
    let across = right(heading);
    let hips = flat(animated.position(RigBone::Hips));
    let ankles = Side::BOTH.map(|side| flat(animated.position(ankle_bone(side))));
    let along = ankles
        .iter()
        .map(|ankle| (*ankle - hips).dot(ahead))
        .sum::<f32>()
        * 0.5;
    let centre = hips + ahead * along;
    let half = config.width_m.max(0.0) * 0.5;
    let toe_out = config.toe_out_deg.to_radians();
    Side::BOTH.map(|side| {
        let (offset, turn) = match side {
            Side::Left => (-half, toe_out),
            Side::Right => (half, -toe_out),
        };
        let ankle = centre + across * offset;
        let length = (flat(animated.position(toe_bone(side))) - ankles[side.index()]).length();
        FootPlacement {
            ankle,
            toe: ankle + forward(wrap(heading + turn)) * length,
        }
    })
}

/// How much of the stance acts: followed toward one while Rico stands still and toward zero
/// otherwise.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct StanceBlend {
    weight: f32,
}

impl StanceBlend {
    pub(super) const fn new() -> Self {
        Self { weight: 0.0 }
    }

    /// Follow toward one while `standing` and zero otherwise over `dt` seconds, and return the
    /// weight.
    pub(super) fn step(&mut self, standing: bool, dt: f32, blend_s: f32) -> f32 {
        let goal = if standing { 1.0 } else { 0.0 };
        self.weight += (goal - self.weight) * follow_factor(dt, blend_s);
        if !self.weight.is_finite() {
            self.weight = 0.0;
        }
        self.weight = self.weight.clamp(0.0, 1.0);
        self.weight
    }

    pub(super) fn reset(&mut self) {
        self.weight = 0.0;
    }
}

/// Move the ankle and toe targets of the `MAIN` pass of `hik` across the ground toward
/// `placements` by `weight`, keeping each target's height. A foot effector the game queued no live
/// target for this tick gets one at the animated pose's height. Returns how many targets moved.
///
/// # Safety
///
/// `hik` must be the live local character's solver, inside its pose-finalization pass before the
/// `MAIN` solve, on the thread running it; `rig` must be the rig of its skeleton and `animated` its
/// pose this tick.
pub(super) unsafe fn place_feet(
    hik: &mut HumanIK,
    rig: &Rig,
    animated: &RigPose,
    placements: &[FootPlacement; 2],
    weight: f32,
) -> usize {
    if weight <= 0.0 || !weight.is_finite() {
        return 0;
    }
    let mut moved = 0;
    for side in Side::BOTH {
        let placement = placements[side.index()];
        for (bone, place) in [
            (ankle_bone(side), placement.ankle),
            (toe_bone(side), placement.toe),
        ] {
            let index = rig.skeleton_index(bone);
            let Ok(index) = u32::try_from(index) else {
                continue;
            };
            let effector = unsafe { hik.GetEffectorIdFromBoneIndex(index) };
            if effector < 0 || effector as u64 >= HumanIK::EFFECTOR_SLOTS {
                continue;
            }
            let animated = animated.position(bone);
            let queued = unsafe { live_target(hik, effector) };
            let from = queued.unwrap_or(animated);
            let flat = Vec2::new(from.x, from.z).lerp(place, weight);
            let target = Vector3 {
                data: [flat.x, from.y, flat.y],
            };
            if !target.data.iter().all(|v| v.is_finite()) {
                continue;
            }
            unsafe {
                hik.AddEffectorTargetPosition(
                    effector,
                    &target,
                    SolveStep::SPINE_HEAD_LOWER_BODY,
                    Pass::MAIN,
                    false,
                    LOCO_IK_INTERPOLATION_RATE,
                    false,
                    LOCO_IK_BLEND_OUT_RATE,
                );
            }
            if queued.is_none() {
                hik.m_TargetReachT[effector as usize] = 1.0;
                hik.m_TargetPull[effector as usize] = 0.0;
            }
            moved += 1;
        }
    }
    moved
}

/// The interpolation and blend-out rates the game's locomotion IK task queues its foot targets
/// with; both are off, so the rates only keep the entry as the game writes it.
const LOCO_IK_INTERPOLATION_RATE: f32 = 3.0;
const LOCO_IK_BLEND_OUT_RATE: f32 = 1.5;

fn ankle_bone(side: Side) -> RigBone {
    match side {
        Side::Left => RigBone::LeftFoot,
        Side::Right => RigBone::RightFoot,
    }
}

fn toe_bone(side: Side) -> RigBone {
    match side {
        Side::Left => RigBone::LeftToeBase,
        Side::Right => RigBone::RightToeBase,
    }
}

/// The position of the live `MAIN` position target on `effector`, model space, when the pass has
/// one queued this tick.
///
/// # Safety
///
/// As [`place_feet`].
unsafe fn live_target(hik: &HumanIK, effector: i32) -> Option<Vec3> {
    let positions = &hik.m_PassInfo[Pass::MAIN as usize].m_EffectorTargetPositions;
    unsafe { positions.as_slice() }
        .iter()
        .find(|entry| entry.effector == effector && entry.is_valid)
        .map(|entry| Vec3::from_array(entry.effector_position.data))
}

#[cfg(test)]
mod tests {
    use body_rig::angle::{line_yaw, yaw_of_flat};
    use glam::Quat;

    use super::*;
    use crate::controllers::body::procedural::rig::tests::skeleton;

    fn pose() -> RigPose {
        RigPose {
            bones: Rig::from_skeleton(&skeleton()).unwrap().reference,
        }
    }

    /// The reference pose with the legs turned about the hips by `turn` and the left foot carried
    /// `ahead` metres forward, as a bladed clip stands.
    fn bladed(turn: f32, ahead: f32) -> RigPose {
        let mut pose = pose();
        let hips = pose.position(RigBone::Hips);
        for leg in [RigBone::LeftUpLeg, RigBone::RightUpLeg] {
            pose.rotate_about(leg, Quat::from_rotation_y(turn), hips);
        }
        let knee = pose.position(RigBone::LeftLeg);
        let foot = pose.position(RigBone::LeftFoot);
        let swing = Quat::from_rotation_arc(
            (foot - knee).normalize(),
            (foot + Vec3::NEG_Z * ahead - knee).normalize(),
        );
        pose.rotate_about(RigBone::LeftLeg, swing, knee);
        pose
    }

    #[test]
    fn the_stance_is_square_across_the_heading_and_as_wide_as_asked() {
        let config = StanceConfig::new();
        for heading in [0.0f32, -0.4, 0.9] {
            let feet = square_stance(&bladed(-0.7, 0.2), heading, &config);
            let across = feet[1].ankle - feet[0].ankle;
            assert!((across.length() - config.width_m).abs() < 1e-4);
            let faces = line_yaw(
                Vec3::new(feet[0].ankle.x, 0.0, feet[0].ankle.y),
                Vec3::new(feet[1].ankle.x, 0.0, feet[1].ankle.y),
            )
            .unwrap();
            assert!(wrap(faces - heading).abs() < 1e-4, "{heading}: {faces}");
        }
    }

    #[test]
    fn the_toes_turn_out_by_the_toe_out() {
        let config = StanceConfig::new();
        let heading = 0.3;
        let feet = square_stance(&pose(), heading, &config);
        let out = config.toe_out_deg.to_radians();
        let left = yaw_of_flat(feet[0].toe - feet[0].ankle).unwrap();
        let right = yaw_of_flat(feet[1].toe - feet[1].ankle).unwrap();
        // A positive heading turns toward the left.
        assert!((wrap(left - heading) - out).abs() < 1e-4);
        assert!((wrap(right - heading) + out).abs() < 1e-4);
    }

    #[test]
    fn the_stance_stays_under_the_hips_and_where_the_clip_stands_along_the_heading() {
        let animated = bladed(-0.7, 0.25);
        let heading = 0.0;
        let feet = square_stance(&animated, heading, &StanceConfig::new());
        let flat = |v: Vec3| Vec2::new(v.x, v.z);
        let hips = flat(animated.position(RigBone::Hips));
        let centre = (feet[0].ankle + feet[1].ankle) * 0.5;
        assert!((centre - hips).dot(right(heading)).abs() < 1e-4);
        let clip = (flat(animated.position(RigBone::LeftFoot))
            + flat(animated.position(RigBone::RightFoot)))
            * 0.5;
        let ahead = forward(heading);
        assert!(((centre - hips).dot(ahead) - (clip - hips).dot(ahead)).abs() < 1e-4);
    }

    #[test]
    fn the_blend_comes_in_standing_and_goes_out_moving() {
        let mut blend = StanceBlend::new();
        let dt = 1.0 / 60.0;
        let mut weight = 0.0;
        for _ in 0..60 {
            weight = blend.step(true, dt, 0.25);
        }
        assert!(weight > 0.95, "{weight}");
        let first = blend.step(false, dt, 0.25);
        assert!(first < weight && first > 0.8, "no snap out: {first}");
        for _ in 0..60 {
            weight = blend.step(false, dt, 0.25);
        }
        assert!(weight < 0.05, "{weight}");
        assert!(blend.step(true, f32::NAN, 0.25).is_finite());
    }
}
