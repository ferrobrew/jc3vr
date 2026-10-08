//! The procedural body's solver: Rico's body built under the player's head. Pure, and in Rico's
//! model space (+Y up, facing -Z, the left side at -X, the root at the feet).
//!
//! The solve starts from the animated pose and changes it in order:
//!
//! 0. The clip's own shape is settled out ([`settle`]): the hips' pitch and roll, which a run
//!    swings with every step, and most of the spine's bend, which the clip's posture carries,
//!    give way to the reference pose's, so the body's shape is the player's rather than the
//!    clip's. The walk's sway comes from the hips' draw toward the planted foot instead.
//! 1. The hips ([`place_hips`]): their height follows the head down (the legs take a drop first,
//!    to a deep squat), and they sit over the stance the root carries ([`hips_horizontal`]), not
//!    over the feet, which leave it with every step, pulled back as the head leans forward so a
//!    lean becomes a hip hinge, and drawn a little toward the one foot on the ground while the
//!    other swings. The caller follows that place over time. They turn to the player's torso,
//!    within a limit. They drop further when a foot is out of the legs' reach ([`reach_drop`]): a
//!    stride, a slope, a stair.
//! 2. The spine ([`bend_spine`]): the chest twists to the torso and pitches forward by a share of
//!    the head's look down (so looking at the belt shows the belt), the spine curls forward when
//!    the head is closer to the hips than the spine is long (a squat past the legs' share), and
//!    the spine swings so the neck reaches the point under the player's head, the swing shared out
//!    along the four spine bones. The neck then takes a share of the head's turn against the chest
//!    ([`turn_neck`]), so a nod or a look aside bends the neck as well as the head.
//! 3. The legs ([`two_bone::solve`]): each foot goes back to its target, the knee bending toward
//!    the foot's forward blended with the hips', never backward, and takes the foot's own
//!    orientation.
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
    /// The middle of the neutral stance the root carries, model space: where the hips stand over.
    pub support: Vec3,
    /// Which feet are on the ground, left then right: with one down and one swinging, the hips are
    /// drawn a little toward the one down.
    pub planted: [bool; 2],
    /// How far each heel is lifted about its toes, radians, left then right.
    pub heel_lifts: [f32; 2],
    /// Where the hips sit horizontally, model space `(x, z)`, as the caller followed
    /// [`hips_horizontal`] over time; `None` takes it as it is this tick.
    pub hips_xz: Option<Vec2>,
    /// How far the hips drop below where the head puts them so the legs reach the feet, metres:
    /// [`reach_drop`], followed over time by [`DropFollower`].
    pub hips_drop: f32,
}

/// Solve the body.
pub fn solve(rig: &Rig, input: &SolveInput, config: &ProceduralBodyConfig) -> RigPose {
    let mut pose = input.animated;
    let torso_yaw = input.torso_yaw.filter(|y| y.is_finite()).unwrap_or(0.0);
    settle(rig, &mut pose);
    place_hips(
        rig,
        &mut pose,
        &hips_input(rig, input, config, input.hips_drop),
        config,
    );
    let neck_target = input.head
        + input.head_rotation
            * (input.animated.position(RigBone::Neck) - input.animated.position(RigBone::Head));
    let chest_limit = config.chest_turn_limit_deg.max(0.0).to_radians();
    let chest_yaw = torso_yaw.clamp(-chest_limit, chest_limit);
    let chest = ChestTurn {
        yaw: chest_yaw,
        pitch: chest_pitch(input.head_rotation),
    };
    bend_spine(&mut pose, neck_target, chest, config);
    turn_neck(rig, &mut pose, input.head_rotation);
    let hips_forward = line_yaw(
        pose.position(RigBone::LeftUpLeg),
        pose.position(RigBone::RightUpLeg),
    )
    .map_or(Vec3::NEG_Z, |yaw| Quat::from_rotation_y(yaw) * Vec3::NEG_Z);
    for (side, (up, leg, foot)) in LEGS.into_iter().enumerate() {
        let pole = knee_pole(rig, foot, input.foot_rotations[side], hips_forward);
        let lift = heel_lift(
            rig,
            side,
            input.feet[side],
            input.foot_rotations[side],
            input.heel_lifts[side],
        );
        place_leg(&mut pose, [up, leg, foot], lift.ankle, lift.rotation, pole);
        // The toes stay flat on the ground while the heel rolls up over them.
        let toe = TOES[side];
        let pivot = pose.position(toe);
        pose.rotate_about(toe, lift.turn.inverse(), pivot);
    }
    if pose.is_finite() {
        pose
    } else {
        input.animated
    }
}

/// Where the hips want to sit horizontally, model space `(x, z)`: over the middle of the stance the
/// root carries, moved back by `counterbalance` of the head's lean ahead of where a neutral head
/// would be over it, the lean measured against the root and taken at most [`MAX_LEAN_M`], and
/// drawn [`PLANTED_BIAS`] of the way toward the one foot on the ground while the other swings.
pub fn hips_horizontal(rig: &Rig, input: &SolveInput, config: &ProceduralBodyConfig) -> Vec2 {
    let reference_hips = rig.reference(RigBone::Hips).translation;
    let head_over_hips = rig.reference(RigBone::Head).translation - reference_hips;
    let support = Vec2::new(input.support.x, input.support.z);
    let yaw = hips_yaw(input, config);
    let neutral = Quat::from_rotation_y(yaw) * Vec3::new(head_over_hips.x, 0.0, head_over_hips.z);
    let lean = Vec2::new(input.head.x, input.head.z) - support - Vec2::new(neutral.x, neutral.z);
    let lean = lean.clamp_length_max(MAX_LEAN_M);
    let counterbalance = config.counterbalance.clamp(0.0, 1.0);
    let bias = match input.planted {
        [true, false] => Some(0),
        [false, true] => Some(1),
        _ => None,
    }
    .map_or(Vec2::ZERO, |i| {
        let foot = input.feet[i];
        Vec2::new(foot.x - support.x, 0.0) * PLANTED_BIAS
    });
    // A squat sends the hips back over the heels as they go down, as a person's do to balance.
    let squat = (reference_hips.y - hips_height(rig, input.head.y, config)).max(0.0);
    let back = Quat::from_rotation_y(yaw) * Vec3::Z * (squat * SQUAT_BACK);
    let place = support - lean * counterbalance + bias + Vec2::new(back.x, back.z);
    if place.is_finite() { place } else { support }
}

/// Follows the hips' horizontal place with a time constant, so a quick lean bends the spine before
/// the hips go with it.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HipsFollower {
    current: Option<Vec2>,
}

impl HipsFollower {
    pub const fn new() -> Self {
        Self { current: None }
    }

    pub fn reset(&mut self) {
        self.current = None;
    }

    /// Start from `place`: the hips start where something else left them.
    pub fn seed(&mut self, place: Vec2) {
        self.current = place.is_finite().then_some(place);
    }

    /// Move toward `wanted` over `dt` seconds with the time constant `time_s`, and return the
    /// place. The first step takes `wanted` as it is.
    pub fn step(&mut self, wanted: Vec2, dt: f32, time_s: f32) -> Vec2 {
        if !wanted.is_finite() {
            return self.current.unwrap_or(Vec2::ZERO);
        }
        let next = match self.current {
            Some(current) if time_s > 0.0 => {
                let k = 1.0 - (-dt.max(0.0) / time_s).exp();
                current + (wanted - current) * k
            }
            _ => wanted,
        };
        self.current = Some(next);
        next
    }
}

/// How far the hips must drop below where the head puts them for both legs to reach their feet,
/// metres: zero when they reach, and never more than the hips have left above the lowest they go.
pub fn reach_drop(rig: &Rig, input: &SolveInput, config: &ProceduralBodyConfig) -> f32 {
    let mut pose = input.animated;
    place_hips(rig, &mut pose, &hips_input(rig, input, config, 0.0), config);
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
    let room = hips_room(rig, input.head.y, config);
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

/// The most the hips may drop below where a head at `head_height` puts them: down to their lowest,
/// counted from where the head has already taken them, so a crouch's drop and a stride's do not
/// add past the floor of a squat.
pub fn hips_room(rig: &Rig, head_height: f32, config: &ProceduralBodyConfig) -> f32 {
    let lowest =
        rig.reference(RigBone::Hips).translation.y * config.min_hips_height.clamp(0.1, 1.0);
    (hips_height(rig, head_height, config) - lowest).max(0.0)
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

/// The most the head's lean counts for the hips, metres: a head further out is a reach, not a lean
/// the hips should hinge back from.
pub const MAX_LEAN_M: f32 = 0.5;

/// How far back the hips go per metre they drop below standing.
pub const SQUAT_BACK: f32 = 0.3;

/// How far toward the one foot on the ground the hips are drawn while the other swings, as a share
/// of that foot's sideways offset from the stance's middle: a walk's small sway over the stance
/// foot.
pub const PLANTED_BIAS: f32 = 0.15;

/// What the hips are placed from.
struct HipsInput {
    head: Vec3,
    horizontal: Vec2,
    yaw: f32,
    drop: f32,
}

fn hips_input(
    rig: &Rig,
    input: &SolveInput,
    config: &ProceduralBodyConfig,
    drop: f32,
) -> HipsInput {
    HipsInput {
        head: input.head,
        horizontal: input
            .hips_xz
            .filter(|h| h.is_finite())
            .unwrap_or_else(|| hips_horizontal(rig, input, config)),
        yaw: hips_yaw(input, config),
        drop: if drop.is_finite() { drop.max(0.0) } else { 0.0 },
    }
}

/// The hips' turn toward the player's torso, within its limit.
fn hips_yaw(input: &SolveInput, config: &ProceduralBodyConfig) -> f32 {
    let torso_yaw = input.torso_yaw.filter(|y| y.is_finite()).unwrap_or(0.0);
    let pelvis_limit = config.pelvis_turn_limit_deg.max(0.0).to_radians();
    torso_yaw.clamp(-pelvis_limit, pelvis_limit)
}

/// Place and turn the hips. See the module docs.
fn place_hips(rig: &Rig, pose: &mut RigPose, input: &HipsInput, config: &ProceduralBodyConfig) {
    let HipsInput {
        head,
        horizontal,
        yaw,
        drop,
    } = *input;
    let height = hips_height(rig, head.y, config) - drop;
    let target = Vec3::new(horizontal.x, height, horizontal.y);

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
    chest: ChestTurn,
    config: &ProceduralBodyConfig,
) {
    let chest_yaw = chest.yaw;
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
    let curled = curl_spine(pose, neck_target, chest_yaw);
    // The chest's pitch bends the upper spine, and like the curl, the whole chain then turns back
    // onto the neck's line about its base, so the bend stays in the chest.
    if chest.pitch > 0.0 {
        let axis = Quat::from_rotation_y(chest_yaw) * Vec3::NEG_X;
        for (bone, share) in SPINE.iter().zip(PITCH_SHARES) {
            let pivot = pose.position(*bone);
            pose.rotate_about(
                *bone,
                Quat::from_axis_angle(axis, chest.pitch * share),
                pivot,
            );
        }
    }
    if (curled || chest.pitch > 0.0)
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

/// A foot rolled up onto its toes.
struct HeelLift {
    /// Where the ankle goes.
    ankle: Vec3,
    /// How the foot is turned.
    rotation: Quat,
    /// The roll, model space: the toes take it back.
    turn: Quat,
}

/// The foot on `side`, its ankle at `ankle` turned `rotation`, rolled up about its toes by `lift`
/// radians: the toes stay where they were and the heel rises.
fn heel_lift(rig: &Rig, side: usize, ankle: Vec3, rotation: Quat, lift: f32) -> HeelLift {
    let flat = HeelLift {
        ankle,
        rotation,
        turn: Quat::IDENTITY,
    };
    if !(lift.is_finite() && lift > 1e-4) {
        return flat;
    }
    let (_, _, foot) = LEGS[side];
    let reference = rig.reference(foot);
    let toes = rig.reference(TOES[side]).translation - reference.translation;
    let carried = rotation * reference.rotation.inverse();
    let pivot = ankle + carried * toes;
    let axis = carried * Vec3::X;
    let turn = Quat::from_axis_angle(axis, -lift);
    let lifted = HeelLift {
        ankle: pivot + turn * (ankle - pivot),
        rotation: (turn * rotation).normalize(),
        turn,
    };
    if lifted.ankle.is_finite() && lifted.rotation.is_finite() {
        lifted
    } else {
        flat
    }
}

/// The toes of each leg, left then right.
const TOES: [RigBone; 2] = [RigBone::LeftToeBase, RigBone::RightToeBase];

/// Where a knee points: the foot's forward, from its orientation against the reference pose's
/// (which faces -Z), blended evenly with the hips' forward, on the ground plane.
fn knee_pole(rig: &Rig, foot: RigBone, foot_rotation: Quat, hips_forward: Vec3) -> Vec3 {
    let turn = foot_rotation * rig.reference(foot).rotation.inverse();
    let foot_forward = turn * Vec3::NEG_Z;
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).normalize_or_zero();
    let pole = flat(foot_forward) + flat(hips_forward);
    if pole.length_squared() > 1e-6 && pole.is_finite() {
        pole.normalize()
    } else {
        flat(hips_forward)
    }
}

/// Bring a leg's foot to `target`, keeping the foot's model-space orientation `foot_rotation`, the
/// knee bending toward `pole`.
fn place_leg(
    pose: &mut RigPose,
    [up, leg, foot]: [RigBone; 3],
    target: Vec3,
    foot_rotation: Quat,
    pole: Vec3,
) {
    let root = pose.position(up);
    let middle = pose.position(leg);
    let end = pose.position(foot);
    let rotations = two_bone::solve(root, middle, end, target, pole);
    pose.rotate_about(up, rotations.upper, root);
    let knee = pose.position(leg);
    pose.rotate_about(leg, rotations.lower, knee);
    let mut placed = *pose.get(foot);
    placed.rotation = foot_rotation;
    pose.set(foot, placed);
}

/// The chest's turn the spine bends to: its yaw toward the torso, and its pitch forward.
#[derive(Debug, Copy, Clone, PartialEq)]
struct ChestTurn {
    /// Radians about +Y.
    yaw: f32,
    /// Radians forward, zero or more.
    pitch: f32,
}

/// Settle the clip's own shape out of `pose`: the hips' pitch and roll go entirely, keeping their
/// turn about +Y, and the spine and the neck go [`SPINE_SETTLE`] of the way to the reference pose's
/// shape on the hips, so a run's bob and the idle's slouch are not the body's.
fn settle(rig: &Rig, pose: &mut RigPose) {
    let hips = *pose.get(RigBone::Hips);
    let delta = hips.rotation * rig.reference(RigBone::Hips).rotation.inverse();
    let swing = delta * yaw_part(delta).inverse();
    if swing.is_finite() {
        pose.rotate_about(RigBone::Hips, swing.inverse(), hips.translation);
    }
    for bone in [
        RigBone::Spine,
        RigBone::Spine1,
        RigBone::Spine2,
        RigBone::Sternum,
        RigBone::Neck,
    ] {
        let Some(parent) = bone.parent() else {
            continue;
        };
        let reference_local =
            rig.reference(parent).rotation.inverse() * rig.reference(bone).rotation;
        let wanted = pose.get(parent).rotation * reference_local;
        let current = pose.get(bone).rotation;
        let full = (wanted * current.inverse()).normalize();
        let part = Quat::IDENTITY.slerp(full, SPINE_SETTLE);
        if part.is_finite() {
            let pivot = pose.position(bone);
            pose.rotate_about(bone, part, pivot);
        }
    }
}

/// The turn of `rotation` about +Y alone: the twist part of its swing-twist split.
fn yaw_part(rotation: Quat) -> Quat {
    let length = (rotation.y * rotation.y + rotation.w * rotation.w).sqrt();
    if length > 1e-6 && length.is_finite() {
        Quat::from_xyzw(0.0, rotation.y / length, 0.0, rotation.w / length)
    } else {
        Quat::IDENTITY
    }
}

/// How far the chest pitches forward for a head turned by `head_rotation` (body-relative): a share
/// of how far the head looks down, at most [`MAX_CHEST_PITCH`]. Looking up leaves it.
fn chest_pitch(head_rotation: Quat) -> f32 {
    let forward = head_rotation * Vec3::NEG_Z;
    let down = (-forward.y).clamp(-1.0, 1.0).asin();
    if down.is_finite() && down > 0.0 {
        (down * CHEST_PITCH_SHARE).min(MAX_CHEST_PITCH)
    } else {
        0.0
    }
}

/// Turn the neck by [`NECK_SHARE`] of the head's turn against the chest, so the head is not the
/// only bone that moves for a look. The head override sets the head itself after the solve.
fn turn_neck(rig: &Rig, pose: &mut RigPose, head_rotation: Quat) {
    let chest =
        pose.get(RigBone::Sternum).rotation * rig.reference(RigBone::Sternum).rotation.inverse();
    let relative = chest.inverse() * head_rotation;
    let share = Quat::IDENTITY.slerp(relative.normalize(), NECK_SHARE);
    let turn = (chest * share * chest.inverse()).normalize();
    if turn.is_finite() {
        let pivot = pose.position(RigBone::Neck);
        pose.rotate_about(RigBone::Neck, turn, pivot);
    }
}

/// How much of the reference pose's spine shape the clip's gives way to.
const SPINE_SETTLE: f32 = 0.8;

/// The share of the head's look down the chest pitches by.
const CHEST_PITCH_SHARE: f32 = 0.35;

/// The furthest the chest pitches forward for a look down, radians: about twenty-five degrees.
const MAX_CHEST_PITCH: f32 = 0.44;

/// Each spine bone's share of the chest's pitch: the upper back takes it.
const PITCH_SHARES: [f32; 4] = [0.0, 0.2, 0.4, 0.4];

/// The share of the head's turn against the chest the neck takes.
const NECK_SHARE: f32 = 0.3;

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
            support: (animated.position(RigBone::LeftFoot) + animated.position(RigBone::RightFoot))
                * 0.5,
            planted: [true; 2],
            heel_lifts: [0.0; 2],
            hips_xz: None,
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
    fn a_step_does_not_move_the_hips() {
        let (rig, mut input, config) = setup();
        let before = hips_horizontal(&rig, &input, &config);
        // One foot lifts and swings ahead: the stance the root carries has not moved.
        input.feet[0].z -= 0.4;
        input.feet[0].y += 0.1;
        let during = hips_horizontal(&rig, &input, &config);
        assert!(during.abs_diff_eq(before, 1e-6), "{before} -> {during}");
        // With the other foot down alone, the hips lean over it, and only a little.
        input.planted = [false, true];
        let swaying = hips_horizontal(&rig, &input, &config);
        assert!(swaying.x > before.x && swaying.x - before.x < 0.03);
        assert!((swaying.y - before.y).abs() < 1e-6);
    }

    #[test]
    fn a_far_reach_counts_only_as_a_capped_lean() {
        let (rig, mut input, config) = setup();
        input.head.z -= 3.0;
        let place = hips_horizontal(&rig, &input, &config);
        assert!(
            (place.y - MAX_LEAN_M * config.counterbalance).abs() < 1e-4,
            "{place}"
        );
    }

    #[test]
    fn the_hips_follower_eases_toward_its_place() {
        let mut follower = HipsFollower::new();
        assert_eq!(follower.step(Vec2::X, 0.05, 0.2), Vec2::X);
        let next = follower.step(Vec2::ZERO, 0.05, 0.2);
        assert!(next.x > 0.5 && next.x < 1.0);
        assert_eq!(follower.step(Vec2::NAN, 0.05, 0.2), next);
    }

    #[test]
    fn a_crouch_leaves_the_hips_less_room_to_drop() {
        let (rig, mut input, config) = setup();
        let standing = hips_room(&rig, input.head.y, &config);
        assert!((standing - 0.5).abs() < 1e-4, "{standing}");
        input.head.y -= 0.3;
        assert!((hips_room(&rig, input.head.y, &config) - 0.2).abs() < 1e-4);
        input.head.y -= 1.0;
        assert_eq!(hips_room(&rig, input.head.y, &config), 0.0);
        // A long stride in the crouch drops the hips no lower than their floor.
        input.head.y += 1.0;
        input.feet[0].z -= 0.6;
        input.feet[1].z += 0.6;
        input.hips_drop = reach_drop(&rig, &input, &config);
        assert!(input.hips_drop <= 0.2 + 1e-4);
        let pose = solve(&rig, &input, &config);
        assert!(pose.position(RigBone::Hips).y >= 0.5 - 1e-3);
    }

    #[test]
    fn looking_down_pitches_the_chest_and_the_neck_takes_a_share() {
        let (rig, input, config) = setup();
        let level = solve(&rig, &input, &config);
        let mut down = input;
        down.head_rotation = Quat::from_rotation_x(-0.8);
        let looking = solve(&rig, &down, &config);
        // The chest bends forward against the lower back.
        let bend = |pose: &RigPose| {
            let chest = (pose.position(RigBone::Neck) - pose.position(RigBone::Spine2)).normalize();
            let lower =
                (pose.position(RigBone::Spine1) - pose.position(RigBone::Spine)).normalize();
            chest.z - lower.z
        };
        assert!(
            bend(&looking) < bend(&level) - 0.05,
            "the chest did not pitch forward: {} vs {}",
            bend(&looking),
            bend(&level)
        );
        let neck_turn = looking
            .get(RigBone::Neck)
            .rotation
            .angle_between(level.get(RigBone::Neck).rotation);
        assert!(neck_turn > 0.1, "{neck_turn}");
        // Looking up leaves the chest.
        assert_eq!(chest_pitch(Quat::from_rotation_x(0.6)), 0.0);
    }

    #[test]
    fn a_lifted_heel_rolls_the_foot_over_flat_toes() {
        let (rig, mut input, config) = setup();
        input.heel_lifts = [0.4, 0.0];
        // A long stride back, so the leg reaches.
        input.feet[0].z += 0.2;
        input.hips_drop = reach_drop(&rig, &input, &config);
        let flat = {
            let mut i = input;
            i.heel_lifts = [0.0; 2];
            solve(&rig, &i, &config)
        };
        let pose = solve(&rig, &input, &config);
        let toes = |p: &RigPose| p.position(RigBone::LeftToeBase);
        assert!(toes(&pose).distance(toes(&flat)) < 2e-3, "the toes moved");
        assert!(pose.position(RigBone::LeftFoot).y > flat.position(RigBone::LeftFoot).y + 0.03);
        let toe_turn = pose
            .get(RigBone::LeftToeBase)
            .rotation
            .angle_between(flat.get(RigBone::LeftToeBase).rotation);
        assert!(toe_turn < 1e-3, "the toes did not stay flat: {toe_turn}");
    }

    #[test]
    fn a_squat_sends_the_hips_back() {
        let (rig, mut input, config) = setup();
        let standing = hips_horizontal(&rig, &input, &config);
        input.head.y -= 0.4;
        let squatting = hips_horizontal(&rig, &input, &config);
        assert!(
            (squatting.y - standing.y - 0.4 * SQUAT_BACK).abs() < 1e-4,
            "{squatting}"
        );
    }

    #[test]
    fn the_clips_hip_pitch_and_spine_bend_are_settled_out() {
        let (rig, mut input, config) = setup();
        let hips = input.animated.position(RigBone::Hips);
        input
            .animated
            .rotate_about(RigBone::Hips, Quat::from_rotation_x(0.3), hips);
        let spine = input.animated.position(RigBone::Spine1);
        input
            .animated
            .rotate_about(RigBone::Spine1, Quat::from_rotation_x(0.4), spine);
        let mut pose = input.animated;
        settle(&rig, &mut pose);
        let hips_tilt = pose
            .get(RigBone::Hips)
            .rotation
            .angle_between(rig.reference(RigBone::Hips).rotation);
        assert!(hips_tilt < 1e-4, "{hips_tilt}");
        let bend = pose
            .get(RigBone::Spine1)
            .rotation
            .angle_between(pose.get(RigBone::Spine).rotation);
        assert!(bend < 0.4 * (1.0 - SPINE_SETTLE) + 1e-3, "{bend}");
        // The solve from it still stands the reference pose up.
        let solved = solve(&rig, &input, &config);
        assert!(solved.is_finite());
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
