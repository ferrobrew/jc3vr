//! The procedural body's solver: Rico's body built under the player's head. Pure, and in Rico's
//! model space (+Y up, facing -Z, the left side at -X, the root at the feet).
//!
//! The solve starts from the animated pose and changes it in order:
//!
//! 1. The hips ([`place_hips`]): their height follows the head down (the legs take a drop first,
//!    to a deep squat), and they sit over the feet's support, pulled back as the head leans forward
//!    so a lean becomes a hip hinge. They turn to the player's torso, within a limit. They drop
//!    further when a foot is out of the legs' reach ([`reach_drop`]): a stride, a slope, a stair.
//! 2. The spine ([`bend_spine`]): the chest twists to the torso, the spine curls forward when the
//!    head is closer to the hips than the spine is long (a squat past the legs' share), and the
//!    spine swings so the neck reaches the point under the player's head, the swing shared out
//!    along the four spine bones.
//! 3. The legs ([`two_bone::solve`]): each foot goes back to its target, bending the knee in the
//!    plane it already bent in, and takes the foot's own orientation.
//!
//! The clavicles then turn toward the hands ([`clavicles`]), after the spine has placed them.
//!
//! Everything is a rotation composed onto the animated bones or a translation of them, so no bone's
//! rest frame needs to be known.

pub mod air;
pub mod arms;
pub mod clavicles;
pub mod feet;
pub mod gait;
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
    /// How each foot should be turned, model space, left then right.
    pub foot_rotations: [Quat; 2],
    /// How far the hips drop below where the head puts them so the legs reach the feet, metres:
    /// [`reach_drop`], followed over time by [`DropFollower`].
    pub hips_drop: f32,
}

/// Solve the body.
pub fn solve(rig: &Rig, input: &SolveInput, config: &ProceduralBodyConfig) -> RigPose {
    let mut pose = input.animated;
    let torso_yaw = input.torso_yaw.filter(|y| y.is_finite()).unwrap_or(0.0);
    place_hips(
        rig,
        &mut pose,
        &hips_input(input, config, input.hips_drop),
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
    for (side, (up, leg, foot)) in LEGS.into_iter().enumerate() {
        place_leg(
            &mut pose,
            [up, leg, foot],
            input.feet[side],
            input.foot_rotations[side],
        );
    }
    if pose.is_finite() {
        pose
    } else {
        input.animated
    }
}

/// How far the hips must drop below where the head puts them for both legs to reach their feet,
/// metres: zero when they reach, and never more than the hips have left above the lowest they go.
pub fn reach_drop(rig: &Rig, input: &SolveInput, config: &ProceduralBodyConfig) -> f32 {
    let mut pose = input.animated;
    place_hips(rig, &mut pose, &hips_input(input, config, 0.0), config);
    let mut need: f32 = 0.0;
    for (side, (up, leg, foot)) in LEGS.into_iter().enumerate() {
        let hip = pose.position(up);
        let length = (pose.position(leg) - hip).length()
            + (pose.position(foot) - pose.position(leg)).length();
        let rest = (rig.reference(foot).translation - rig.reference(up).translation).length();
        let reach = (length * LEG_REACH).max(rest.min(length));
        let offset = hip - input.feet[side];
        let across = Vec2::new(offset.x, offset.z).length();
        let below = if across < reach {
            offset.y - (reach * reach - across * across).sqrt()
        } else {
            offset.y
        };
        need = need.max(below);
    }
    let standing = rig.reference(RigBone::Hips).translation.y;
    let room = standing * (1.0 - config.min_hips_height.clamp(0.1, 1.0));
    if need.is_finite() {
        need.clamp(0.0, room)
    } else {
        0.0
    }
}

/// Follows the hips' reach drop: down quickly, so a foot on a lower stair reaches it as it lands,
/// and back up at a third of the pace, so the body does not bob on every stride.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct DropFollower {
    current: f32,
}

impl DropFollower {
    pub const fn new() -> Self {
        Self { current: 0.0 }
    }

    pub fn reset(&mut self) {
        self.current = 0.0;
    }

    /// Start from `drop`: the hips start where something else left them and rise from there at the
    /// follower's pace.
    pub fn seed(&mut self, drop: f32) {
        self.current = if drop.is_finite() { drop.max(0.0) } else { 0.0 };
    }

    /// Move toward `need` over `dt` seconds with the time constant `time_s`, and return the drop.
    pub fn step(&mut self, need: f32, dt: f32, time_s: f32) -> f32 {
        let need = if need.is_finite() { need.max(0.0) } else { 0.0 };
        let time = if need > self.current {
            time_s
        } else {
            time_s * RISE_SLOWDOWN
        };
        self.current = if time <= 0.0 {
            need
        } else {
            let k = 1.0 - (-dt.max(0.0) / time).exp();
            self.current + (need - self.current) * k
        };
        self.current
    }
}

/// The hips' height for a head at `head_height`, before any drop for the legs' reach: the head's
/// less the reference pose's head-over-hips height, at most the reference standing height and at
/// least `min_hips_height` of it.
pub fn hips_height(rig: &Rig, head_height: f32, config: &ProceduralBodyConfig) -> f32 {
    let reference_hips = rig.reference(RigBone::Hips).translation;
    let head_over_hips = rig.reference(RigBone::Head).translation.y - reference_hips.y;
    let standing = reference_hips.y;
    let lowest = standing * config.min_hips_height.clamp(0.1, 1.0);
    (head_height - head_over_hips).clamp(lowest, standing)
}

/// The yaw of the horizontal line from `left` to `right` about +Y, measured from +X: the facing of
/// a pair of bones on either side of the body.
pub fn line_yaw(left: Vec3, right: Vec3) -> Option<f32> {
    let line = Vec2::new(right.x - left.x, right.z - left.z);
    (line.length_squared() > 1e-8).then(|| (-line.y).atan2(line.x))
}

/// The legs' bones, left then right: the thigh, the shin, and the foot.
const LEGS: [(RigBone, RigBone, RigBone); 2] = [
    (RigBone::LeftUpLeg, RigBone::LeftLeg, RigBone::LeftFoot),
    (RigBone::RightUpLeg, RigBone::RightLeg, RigBone::RightFoot),
];

/// The share of a leg's length it reaches to before the hips drop: a leg never quite straight.
const LEG_REACH: f32 = 0.97;

/// How much slower the hips rise back than they drop.
const RISE_SLOWDOWN: f32 = 3.0;

/// What the hips are placed from.
struct HipsInput {
    head: Vec3,
    feet: [Vec3; 2],
    yaw: f32,
    drop: f32,
}

fn hips_input(input: &SolveInput, config: &ProceduralBodyConfig, drop: f32) -> HipsInput {
    let torso_yaw = input.torso_yaw.filter(|y| y.is_finite()).unwrap_or(0.0);
    let pelvis_limit = config.pelvis_turn_limit_deg.max(0.0).to_radians();
    HipsInput {
        head: input.head,
        feet: input.feet,
        yaw: torso_yaw.clamp(-pelvis_limit, pelvis_limit),
        drop: if drop.is_finite() { drop.max(0.0) } else { 0.0 },
    }
}

/// Place and turn the hips. See the module docs.
fn place_hips(rig: &Rig, pose: &mut RigPose, input: &HipsInput, config: &ProceduralBodyConfig) {
    let HipsInput {
        head,
        feet,
        yaw,
        drop,
    } = *input;
    let reference_hips = rig.reference(RigBone::Hips).translation;
    let head_over_hips = rig.reference(RigBone::Head).translation - reference_hips;
    let height = hips_height(rig, head.y, config) - drop;

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

/// Twist the chest to `chest_yaw`, curl the spine for a near target, and swing it so the neck
/// reaches `neck_target`.
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
        let Some((axis, angle)) = swing_toward(pose, neck_target, limit) else {
            break;
        };
        for (bone, share) in SPINE.iter().zip(SWING_SHARES) {
            let pivot = pose.position(*bone);
            pose.rotate_about(*bone, Quat::from_axis_angle(axis, angle * share), pivot);
        }
    }
    // The curl moves the neck off the line the swing found, so the whole chain turns back onto
    // it about its base, keeping the curl's shape: a distributed swing would straighten it.
    if curl_spine(pose, neck_target, chest_yaw)
        && let Some((axis, angle)) = swing_toward(pose, neck_target, limit)
    {
        let base = pose.position(RigBone::Spine);
        pose.rotate_about(RigBone::Spine, Quat::from_axis_angle(axis, angle), base);
    }
}

/// The swing that turns the spine's base-to-neck line toward `neck_target`, or `None` when it is
/// already there. A lean past `limit` from upright is taken only as far as the limit: the spine
/// does not fold over.
fn swing_toward(pose: &RigPose, neck_target: Vec3, limit: f32) -> Option<(Vec3, f32)> {
    let base = pose.position(RigBone::Spine);
    let from = pose.position(RigBone::Neck) - base;
    let to = neck_target - base;
    if from.length_squared() < 1e-8 || to.length_squared() < 1e-8 {
        return None;
    }
    let mut direction = to.normalize();
    if direction.angle_between(Vec3::Y) > limit {
        // A target straight below the base has no lean direction of its own; the lean is forward.
        let axis = Vec3::Y
            .cross(direction)
            .try_normalize()
            .unwrap_or(Vec3::NEG_X);
        direction = Quat::from_axis_angle(axis, limit) * Vec3::Y;
    }
    let (axis, angle) = Quat::from_rotation_arc(from.normalize(), direction).to_axis_angle();
    (axis.is_finite() && angle.abs() >= 1e-5).then_some((axis, angle))
}

/// Curl the spine forward, about the chest's sideways axis, until the neck is as far from the
/// spine's base as `neck_target` is: a head lower than the hips' lowest takes the rest of the drop
/// in the spine, as a squat does. A target as far as the spine is long, or further, leaves it
/// alone. Returns whether it curled.
fn curl_spine(pose: &mut RigPose, neck_target: Vec3, chest_yaw: f32) -> bool {
    let base = pose.position(RigBone::Spine);
    let length = (pose.position(RigBone::Neck) - base).length();
    let wanted = (neck_target - base).length();
    if !(length > 1e-4 && wanted < length * CURL_START) {
        return false;
    }
    let axis = Quat::from_rotation_y(chest_yaw) * Vec3::NEG_X;
    let curled = |angle: f32| {
        let mut p = *pose;
        for (bone, share) in SPINE.iter().zip(SWING_SHARES) {
            let pivot = p.position(*bone);
            p.rotate_about(*bone, Quat::from_axis_angle(axis, angle * share), pivot);
        }
        p
    };
    let (mut low, mut high) = (0.0, MAX_CURL);
    for _ in 0..CURL_ITERATIONS {
        let mid = (low + high) * 0.5;
        let reach = (curled(mid).position(RigBone::Neck) - base).length();
        if reach > wanted {
            low = mid;
        } else {
            high = mid;
        }
    }
    *pose = curled((low + high) * 0.5);
    true
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

/// Each spine bone's share of the swing toward the neck target, and of the curl.
const SWING_SHARES: [f32; 4] = [0.2, 0.25, 0.3, 0.25];

/// Each spine bone's share of the chest's twist, more of it higher up as in a real spine.
const TWIST_SHARES: [f32; 4] = [0.15, 0.2, 0.3, 0.35];

/// The swing is applied about the four joints rather than the base, so it is repeated to converge.
const SWING_ITERATIONS: usize = 3;

/// The share of the spine's length below which the head's closeness curls it.
const CURL_START: f32 = 0.99;

/// The furthest the spine curls, radians, shared along its bones: about as far as a back bends.
/// A head lower still leaves the neck short of its target, and the head override closes the rest.
const MAX_CURL: f32 = std::f32::consts::FRAC_PI_3 * 2.0;

const CURL_ITERATIONS: usize = 16;

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
            foot_rotations: [
                animated.get(RigBone::LeftFoot).rotation,
                animated.get(RigBone::RightFoot).rotation,
            ],
            hips_drop: 0.0,
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
    fn a_foot_out_of_reach_drops_the_hips_until_it_reaches() {
        let (rig, mut input, config) = setup();
        assert!(reach_drop(&rig, &input, &config) < 1e-4);
        // A long stride: the front foot ahead and the back foot behind.
        input.feet[0].z -= 0.45;
        input.feet[1].z += 0.45;
        let drop = reach_drop(&rig, &input, &config);
        assert!(drop > 0.02, "{drop}");
        input.hips_drop = drop;
        let pose = solve(&rig, &input, &config);
        for (side, foot) in [RigBone::LeftFoot, RigBone::RightFoot]
            .into_iter()
            .enumerate()
        {
            assert!(
                pose.position(foot).distance(input.feet[side]) < 2e-3,
                "{foot:?} at {:?}",
                pose.position(foot)
            );
        }
    }

    #[test]
    fn a_head_below_the_lowest_hips_curls_the_spine() {
        let (rig, mut input, config) = setup();
        // The hips stop at half their height; the head goes a little further down.
        input.head.y = 1.65 - 0.5 - 0.03;
        let pose = solve(&rig, &input, &config);
        assert!((pose.position(RigBone::Hips).y - 0.5).abs() < 1e-3);
        let neck_target = input.head - Vec3::Y * 0.1;
        let base = pose.position(RigBone::Spine);
        let reach = (pose.position(RigBone::Neck) - base).length();
        assert!(
            (reach - (neck_target - base).length()).abs() < 0.02,
            "the neck is at {:?}",
            pose.position(RigBone::Neck)
        );
        assert!((pose.position(RigBone::Neck) - base).angle_between(neck_target - base) < 0.05);
        // Far lower, the spine curls only as far as it bends.
        input.head.y = 0.5;
        let pose = solve(&rig, &input, &config);
        assert!(pose.is_finite() && pose != input.animated);
        assert!(pose.position(RigBone::Neck).y > input.head.y);
    }

    #[test]
    fn the_drop_follower_drops_fast_and_rises_slowly() {
        let mut follower = DropFollower::new();
        let down = follower.step(0.1, 0.05, 0.05);
        let mut up = DropFollower { current: 0.1 };
        let rise = 0.1 - up.step(0.0, 0.05, 0.05);
        assert!(down > rise * 2.0);
        assert!(follower.step(f32::NAN, 0.05, 0.05) <= down);
    }

    #[test]
    fn non_finite_input_returns_the_animation() {
        let (rig, mut input, config) = setup();
        input.head = Vec3::NAN;
        assert_eq!(solve(&rig, &input, &config), input.animated);
    }
}
