//! The procedural body's solver: a standing body built under the player's head. Pure, and in Rico's
//! model space (+Y up, facing -Z, the left side at -X, the root at the feet).
//!
//! The solve starts from the animated pose and changes it in order:
//!
//! 1. The hips ([`place_hips`]): their height follows the head down (the legs take a drop first,
//!    to a deep squat), and they sit over the feet's support, pulled back as the head leans forward
//!    so a lean becomes a hip hinge. They turn to the player's torso, within a limit.
//! 2. The spine ([`bend_spine`]): the chest twists to the torso, and the spine swings so the neck
//!    reaches the point under the player's head, the swing shared out along the four spine bones.
//! 3. The legs ([`two_bone::solve`]): each foot goes back to its target, bending the knee in the
//!    plane it already bent in, and keeps the foot's own orientation.
//!
//! Everything is a rotation composed onto the animated bones or a translation of them, so no bone's
//! rest frame needs to be known.

pub mod pose;
pub mod two_bone;

use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::procedural::{
    config::ProceduralBodyConfig,
    rig::{Rig, RigBone},
    solver::pose::RigPose,
};

/// One solve's inputs, model space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SolveInput {
    /// The animated pose.
    pub animated: RigPose,
    /// Where the head bone should be: the player's head.
    pub head: Vec3,
    /// The player's head rotation relative to the body (identity faces forward).
    pub head_rotation: Quat,
    /// The player's torso heading relative to Rico's facing, radians about +Y, when estimated.
    pub torso_yaw: Option<f32>,
    /// Where each foot (the ankle) should be, left then right.
    pub feet: [Vec3; 2],
}

/// Solve the standing body.
pub fn solve(rig: &Rig, input: &SolveInput, config: &ProceduralBodyConfig) -> RigPose {
    let mut pose = input.animated;
    let torso_yaw = input.torso_yaw.filter(|y| y.is_finite()).unwrap_or(0.0);
    let pelvis_limit = config.pelvis_turn_limit_deg.max(0.0).to_radians();
    place_hips(
        rig,
        &mut pose,
        input.head,
        input.feet,
        torso_yaw.clamp(-pelvis_limit, pelvis_limit),
        config,
    );
    let neck_target = input.head
        + input.head_rotation
            * (input.animated.position(RigBone::Neck) - input.animated.position(RigBone::Head));
    let chest_limit = config.chest_turn_limit_deg.max(0.0).to_radians();
    bend_spine(
        &mut pose,
        neck_target,
        torso_yaw.clamp(-chest_limit, chest_limit),
        config,
    );
    for (side, (up, leg, foot)) in [
        (RigBone::LeftUpLeg, RigBone::LeftLeg, RigBone::LeftFoot),
        (RigBone::RightUpLeg, RigBone::RightLeg, RigBone::RightFoot),
    ]
    .into_iter()
    .enumerate()
    {
        place_leg(
            &mut pose,
            [up, leg, foot],
            input.feet[side],
            input.animated.get(foot).rotation,
        );
    }
    if pose.is_finite() {
        pose
    } else {
        input.animated
    }
}

/// The yaw of the horizontal line from `left` to `right` about +Y, measured from +X: the facing of
/// a pair of bones on either side of the body.
pub fn line_yaw(left: Vec3, right: Vec3) -> Option<f32> {
    let line = Vec2::new(right.x - left.x, right.z - left.z);
    (line.length_squared() > 1e-8).then(|| (-line.y).atan2(line.x))
}

/// Place and turn the hips. See the module docs.
fn place_hips(
    rig: &Rig,
    pose: &mut RigPose,
    head: Vec3,
    feet: [Vec3; 2],
    yaw: f32,
    config: &ProceduralBodyConfig,
) {
    let reference_hips = rig.reference(RigBone::Hips).translation;
    let head_over_hips = rig.reference(RigBone::Head).translation - reference_hips;
    let standing = reference_hips.y;
    let lowest = standing * config.min_hips_height.clamp(0.1, 1.0);
    let height = (head.y - head_over_hips.y).clamp(lowest, standing);

    let support = (feet[0] + feet[1]) * 0.5;
    let turn = Quat::from_rotation_y(yaw);
    let neutral_head = support + turn * Vec3::new(head_over_hips.x, 0.0, head_over_hips.z);
    let lean = Vec3::new(head.x - neutral_head.x, 0.0, head.z - neutral_head.z);
    let counterbalance = config.counterbalance.clamp(0.0, 1.0);
    let target = Vec3::new(support.x, height, support.z) - lean * counterbalance;

    let hips = pose.position(RigBone::Hips);
    pose.translate(RigBone::Hips, target - hips);

    if let Some(current) = line_yaw(
        pose.position(RigBone::LeftUpLeg),
        pose.position(RigBone::RightUpLeg),
    ) {
        let delta = wrap(yaw - current);
        pose.rotate_about(RigBone::Hips, Quat::from_rotation_y(delta), target);
    }
}

/// Twist the chest to `chest_yaw` and swing the spine so the neck reaches `neck_target`.
fn bend_spine(
    pose: &mut RigPose,
    neck_target: Vec3,
    chest_yaw: f32,
    config: &ProceduralBodyConfig,
) {
    if let Some(current) = line_yaw(
        pose.position(RigBone::LeftShoulder),
        pose.position(RigBone::RightShoulder),
    ) {
        let twist = wrap(chest_yaw - current);
        for (bone, share) in SPINE.iter().zip(TWIST_SHARES) {
            let pivot = pose.position(*bone);
            pose.rotate_about(*bone, Quat::from_rotation_y(twist * share), pivot);
        }
    }
    let limit = config.lean_limit_deg.max(0.0).to_radians();
    for _ in 0..SWING_ITERATIONS {
        let base = pose.position(RigBone::Spine);
        let from = pose.position(RigBone::Neck) - base;
        let to = neck_target - base;
        if from.length_squared() < 1e-8 || to.length_squared() < 1e-8 {
            return;
        }
        // A lean past the limit is taken only as far as the limit: the spine does not fold.
        let mut direction = to.normalize();
        let tilt = direction.angle_between(Vec3::Y);
        if tilt > limit {
            let axis = Vec3::Y.cross(direction).normalize_or_zero();
            if axis != Vec3::ZERO {
                direction = Quat::from_axis_angle(axis, limit) * Vec3::Y;
            }
        }
        let swing = Quat::from_rotation_arc(from.normalize(), direction);
        let (axis, angle) = swing.to_axis_angle();
        if !axis.is_finite() || angle.abs() < 1e-5 {
            return;
        }
        for (bone, share) in SPINE.iter().zip(SWING_SHARES) {
            let pivot = pose.position(*bone);
            pose.rotate_about(*bone, Quat::from_axis_angle(axis, angle * share), pivot);
        }
    }
}

/// Bring a leg's foot to `target`, keeping the foot's model-space orientation `foot_rotation`.
fn place_leg(pose: &mut RigPose, [up, leg, foot]: [RigBone; 3], target: Vec3, foot_rotation: Quat) {
    let root = pose.position(up);
    let middle = pose.position(leg);
    let end = pose.position(foot);
    let forward = Vec3::NEG_Z;
    let rotations = two_bone::solve(root, middle, end, target, forward);
    pose.rotate_about(up, rotations.upper, root);
    let knee = pose.position(leg);
    pose.rotate_about(leg, rotations.lower, knee);
    let mut placed = *pose.get(foot);
    placed.rotation = foot_rotation;
    pose.set(foot, placed);
}

/// The spine bones the bend is shared along, from the base.
const SPINE: [RigBone; 4] = [
    RigBone::Spine,
    RigBone::Spine1,
    RigBone::Spine2,
    RigBone::Sternum,
];

/// Each spine bone's share of the swing toward the neck target.
const SWING_SHARES: [f32; 4] = [0.2, 0.25, 0.3, 0.25];

/// Each spine bone's share of the chest's twist, more of it higher up as in a real spine.
const TWIST_SHARES: [f32; 4] = [0.15, 0.2, 0.3, 0.35];

/// The swing is applied about the four joints rather than the base, so it is repeated to converge.
const SWING_ITERATIONS: usize = 3;

fn wrap(angle: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (angle + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::procedural::rig::tests::skeleton;

    fn setup() -> (Rig, SolveInput, ProceduralBodyConfig) {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let animated = RigPose {
            bones: rig.reference,
        };
        let input = SolveInput {
            animated,
            head: animated.position(RigBone::Head),
            head_rotation: Quat::IDENTITY,
            torso_yaw: Some(0.0),
            feet: [
                animated.position(RigBone::LeftFoot),
                animated.position(RigBone::RightFoot),
            ],
        };
        (rig, input, ProceduralBodyConfig::new())
    }

    #[test]
    fn the_neutral_head_leaves_the_reference_pose_standing() {
        let (rig, input, config) = setup();
        let pose = solve(&rig, &input, &config);
        for bone in RigBone::ALL {
            assert!(
                pose.position(bone)
                    .abs_diff_eq(input.animated.position(bone), 1e-3),
                "{bone:?} moved to {:?}",
                pose.position(bone)
            );
        }
    }

    #[test]
    fn a_lowered_head_bends_the_knees_and_keeps_the_feet() {
        let (rig, mut input, config) = setup();
        input.head.y -= 0.3;
        let pose = solve(&rig, &input, &config);
        assert!((pose.position(RigBone::Hips).y - 0.7).abs() < 1e-3);
        for (side, foot) in [RigBone::LeftFoot, RigBone::RightFoot]
            .into_iter()
            .enumerate()
        {
            assert!(pose.position(foot).abs_diff_eq(input.feet[side], 2e-3));
        }
        // The test legs bend forward at the knee, as the shipped ones do.
        assert!(pose.position(RigBone::LeftLeg).z < input.animated.position(RigBone::LeftLeg).z);
        assert!(
            pose.position(RigBone::Neck)
                .distance(input.head - Vec3::Y * 0.1)
                < 0.02
        );
    }

    #[test]
    fn a_forward_lean_hinges_the_hips_back() {
        let (rig, mut input, config) = setup();
        input.head += Vec3::new(0.0, -0.1, -0.3);
        let pose = solve(&rig, &input, &config);
        assert!(
            pose.position(RigBone::Hips).z > 0.0,
            "the hips did not move back"
        );
        // The target is a little beyond the spine's length, so the neck points at it.
        let neck_target = input.head - Vec3::Y * 0.1;
        let base = pose.position(RigBone::Spine);
        let aim = (pose.position(RigBone::Neck) - base).angle_between(neck_target - base);
        assert!(
            aim.to_degrees() < 3.0,
            "the neck is {} degrees off",
            aim.to_degrees()
        );
    }

    #[test]
    fn the_torso_turn_twists_the_hips_within_their_limit() {
        let (rig, mut input, config) = setup();
        input.torso_yaw = Some(1.2);
        let pose = solve(&rig, &input, &config);
        let hips = line_yaw(
            pose.position(RigBone::LeftUpLeg),
            pose.position(RigBone::RightUpLeg),
        )
        .unwrap();
        assert!((hips - config.pelvis_turn_limit_deg.to_radians()).abs() < 1e-3);
        let chest = line_yaw(
            pose.position(RigBone::LeftShoulder),
            pose.position(RigBone::RightShoulder),
        )
        .unwrap();
        assert!(chest > hips);
    }

    #[test]
    fn non_finite_input_returns_the_animation() {
        let (rig, mut input, config) = setup();
        input.head = Vec3::NAN;
        assert_eq!(solve(&rig, &input, &config), input.animated);
    }
}
