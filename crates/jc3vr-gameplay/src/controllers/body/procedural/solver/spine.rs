//! The spine and the neck: the chest's pitch for a look down, the swing that puts the neck under
//! the player's head, and the shares the spine's bones and the neck take. Pure, in Rico's model
//! space.

use glam::{Quat, Vec3};

use crate::controllers::body::procedural::{rig::RigBone, solver::pose::RigPose};

/// The swing that turns the spine's base-to-neck line toward `neck_target`, or `None` when it is
/// already there. A lean past `limit` from `upright` (a unit direction: +Y on foot, the clip's own
/// spine where the animation keeps the hips) is taken only as far as the limit: the spine does not
/// fold over.
pub(super) fn swing_toward(
    pose: &RigPose,
    neck_target: Vec3,
    upright: Vec3,
    limit: f32,
) -> Option<(Vec3, f32)> {
    let base = pose.position(RigBone::Spine);
    let from = pose.position(RigBone::Neck) - base;
    let to = neck_target - base;
    if from.length_squared() < 1e-8 || to.length_squared() < 1e-8 {
        return None;
    }
    let mut direction = to.normalize();
    if direction.angle_between(upright) > limit {
        // A target straight below the base has no lean direction of its own; the lean is forward.
        let axis = upright
            .cross(direction)
            .try_normalize()
            .unwrap_or(Vec3::NEG_X);
        direction = Quat::from_axis_angle(axis, limit) * upright;
    }
    let (axis, angle) = Quat::from_rotation_arc(from.normalize(), direction).to_axis_angle();
    (axis.is_finite() && angle.abs() >= 1e-5).then_some((axis, angle))
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

/// The share of the head's look down the chest pitches by.
const CHEST_PITCH_SHARE: f32 = 0.35;

/// The furthest the chest pitches forward for a look down, radians: about twenty-five degrees.
const MAX_CHEST_PITCH: f32 = 0.44;

/// Each spine bone's share of the chest's pitch: the upper back takes it.
pub(super) const PITCH_SHARES: [f32; 4] = [0.0, 0.2, 0.4, 0.4];

/// The share of the head's turn against the chest the neck takes.
pub(super) const NECK_SHARE: f32 = 0.3;

/// The spine bones the bend is shared along, from the base.
pub(super) const SPINE: [RigBone; 4] = [
    RigBone::Spine,
    RigBone::Spine1,
    RigBone::Spine2,
    RigBone::Sternum,
];

/// Each spine bone's share of the swing toward the neck target.
pub(super) const SWING_SHARES: [f32; 4] = [0.2, 0.25, 0.3, 0.25];

/// Each spine bone's share of the chest's twist, more of it higher up as in a real spine.
pub(super) const TWIST_SHARES: [f32; 4] = [0.15, 0.2, 0.3, 0.35];

/// The swing is applied about the four joints rather than the base, so it is repeated to converge.
pub(super) const SWING_ITERATIONS: usize = 3;
