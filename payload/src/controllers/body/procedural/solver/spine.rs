//! The spine and the neck: the chest's twist and pitch, the curl of a deep squat, the swing that
//! puts the neck under the player's head, and the neck's share of the head's turn. Pure, in Rico's
//! model space.

use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{
    config::ProceduralBodyConfig,
    rig::{Rig, RigBone},
    solver::{line_yaw, pose::RigPose, wrap},
};

/// The chest's turn the spine bends to: its yaw toward the torso, and its pitch forward.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct ChestTurn {
    /// Radians about +Y.
    pub(super) yaw: f32,
    /// Radians forward, zero or more.
    pub(super) pitch: f32,
}

/// Twist the chest to `chest_yaw`, curl the spine for a near target, and swing it so the neck
/// reaches `neck_target`.
pub(super) fn bend_spine(
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

/// How far the chest pitches forward for a head turned by `head_rotation` (body-relative): a share
/// of how far the head looks down, at most [`MAX_CHEST_PITCH`]. Looking up leaves it.
pub(super) fn chest_pitch(head_rotation: Quat) -> f32 {
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
pub(super) fn turn_neck(rig: &Rig, pose: &mut RigPose, head_rotation: Quat) {
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
