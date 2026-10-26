//! Realize the committed fit on the driven skeleton without another posture estimate.

use glam::{Mat4, Quat, Vec3};

use crate::controllers::body::{
    procedural::{
        rig::{Rig, RigBone},
        solver::{LEGS, place_leg, pose::RigPose, settle, spine::turn_neck},
    },
    rig::estimate::Snapshot,
};

/// The fit owns the pelvis, torso, and inferred feet. Tracking continues to own head orientation.
pub(crate) fn realize(
    rig: &Rig,
    animated: &RigPose,
    snapshot: &Snapshot,
    world: Mat4,
    head_rotation: Quat,
) -> Option<RigPose> {
    let inverse = world.inverse();
    let root_rotation = world.to_scale_rotation_translation().1;
    let point = |point| inverse.transform_point3(point);
    let body = snapshot.body;
    let mut pose = *animated;
    settle(rig, &mut pose);
    let pelvis_rotation = root_rotation.inverse() * body.pelvis_rotation;
    let hips = *pose.get(RigBone::Hips);
    pose.rotate_about(
        RigBone::Hips,
        pelvis_rotation * rig.reference(RigBone::Hips).rotation * hips.rotation.inverse(),
        hips.translation,
    );
    pose.translate(RigBone::Hips, point(body.pelvis) - hips.translation);
    let chest = point(body.chest);
    let neck = point(body.neck);
    let base = pose.position(RigBone::Spine);
    let current_chest =
        (pose.position(RigBone::LeftShoulder) + pose.position(RigBone::RightShoulder)) * 0.5;
    swing(&mut pose, RigBone::Spine, base, current_chest, chest);
    let sternum = pose.position(RigBone::Sternum);
    let current_neck = pose.position(RigBone::Neck);
    swing(&mut pose, RigBone::Sternum, sternum, current_neck, neck);
    let desired_yaw = body.chest_yaw - yaw(root_rotation);
    let current_yaw = crate::controllers::body::procedural::solver::line_yaw(
        pose.position(RigBone::LeftShoulder),
        pose.position(RigBone::RightShoulder),
    )
    .unwrap_or(0.0);
    let pivot = pose.position(RigBone::Sternum);
    pose.rotate_about(
        RigBone::Sternum,
        Quat::from_rotation_y(desired_yaw - current_yaw),
        pivot,
    );
    turn_neck(rig, &mut pose, head_rotation);
    for (side, (up, leg, foot)) in LEGS.into_iter().enumerate() {
        let pole = (point(body.knees[side]) - point(body.hips[side]))
            .try_normalize()
            .unwrap_or(Vec3::NEG_Z);
        place_leg(
            &mut pose,
            [up, leg, foot],
            point(snapshot.feet[side].position),
            root_rotation.inverse() * snapshot.feet[side].rotation * rig.reference(foot).rotation,
            pole,
        );
    }
    pose.is_finite().then_some(pose)
}

fn yaw(rotation: Quat) -> f32 {
    let forward = rotation * Vec3::NEG_Z;
    (-forward.x).atan2(-forward.z)
}

fn swing(pose: &mut RigPose, bone: RigBone, pivot: Vec3, from: Vec3, to: Vec3) {
    if let (Some(from), Some(to)) = ((from - pivot).try_normalize(), (to - pivot).try_normalize()) {
        pose.rotate_about(bone, Quat::from_rotation_arc(from, to), pivot);
    }
}
