//! The SMPL body's kinematic tree and the conventions the estimators share: AMASS's z-up world,
//! the 6D rotation format, and the yaws read off an estimated body.
//!
//! An SMPL joint's frame at rest is the body's: +X to the body's left, +Y up, +Z forward. AMASS's
//! world is z-up, and maps to the room frame (+X right, +Y up, -Z forward) by
//! `amass = (x, -z, y)` ([`AMASS_FROM_ROOM`]), a proper rotation.

use glam::{DMat3, DVec3};

/// The body joints the estimators output: the pelvis and 21 below it, without the hands' fingers.
pub const JOINT_COUNT: usize = 22;

/// Each joint's parent in the SMPL kinematic tree; the pelvis has none.
pub const PARENTS: [Option<usize>; JOINT_COUNT] = [
    None,
    Some(0),
    Some(0),
    Some(0),
    Some(1),
    Some(2),
    Some(3),
    Some(4),
    Some(5),
    Some(6),
    Some(7),
    Some(8),
    Some(9),
    Some(9),
    Some(9),
    Some(12),
    Some(13),
    Some(14),
    Some(16),
    Some(17),
    Some(18),
    Some(19),
];

/// The joints by name, in [`PARENTS`]' order.
pub const NAMES: [&str; JOINT_COUNT] = [
    "pelvis",
    "left_hip",
    "right_hip",
    "spine1",
    "left_knee",
    "right_knee",
    "spine2",
    "left_ankle",
    "right_ankle",
    "spine3",
    "left_foot",
    "right_foot",
    "neck",
    "left_collar",
    "right_collar",
    "head",
    "left_shoulder",
    "right_shoulder",
    "left_elbow",
    "right_elbow",
    "left_wrist",
    "right_wrist",
];

pub const PELVIS: usize = 0;
/// The upper chest, the parent of the neck and both collars.
pub const SPINE3: usize = 9;
pub const HEAD: usize = 15;
pub const LEFT_WRIST: usize = 20;
pub const RIGHT_WRIST: usize = 21;

/// The room frame to AMASS's z-up world: `amass = (x, -z, y)`.
pub const AMASS_FROM_ROOM: DMat3 = DMat3::from_cols(
    DVec3::new(1.0, 0.0, 0.0),
    DVec3::new(0.0, 0.0, 1.0),
    DVec3::new(0.0, -1.0, 0.0),
);

/// A joint's global rotation and position in AMASS's world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct JointPose {
    pub rotation: DMat3,
    pub position: DVec3,
}

/// A rotation in the estimators' 6D format: its first two columns, `[col0, col1]`.
pub fn to_6d(rotation: DMat3) -> [f64; 6] {
    let (a, b) = (rotation.x_axis, rotation.y_axis);
    [a.x, a.y, a.z, b.x, b.y, b.z]
}

/// The rotation a 6D value stands for, made orthonormal by Gram-Schmidt. The reference decoder
/// skips the normalisation; the two agree on a well-formed output and differ in nothing a yaw reads.
pub fn from_6d(value: &[f32]) -> DMat3 {
    let a = DVec3::new(value[0].into(), value[1].into(), value[2].into()).normalize();
    let b = DVec3::new(value[3].into(), value[4].into(), value[5].into());
    let b = (b - a * a.dot(b)).normalize();
    DMat3::from_cols(a, b, a.cross(b))
}

/// Every joint's global rotation in AMASS's world from an estimate's root orientation (6D) and the
/// 21 other joints' rotations relative to their parents (6D each, in joint order).
pub fn global_rotations(root_orient: &[f32; 6], pose_body: &[f32; 126]) -> [DMat3; JOINT_COUNT] {
    let mut global = [DMat3::IDENTITY; JOINT_COUNT];
    global[PELVIS] = from_6d(root_orient);
    for joint in 1..JOINT_COUNT {
        let local = from_6d(&pose_body[(joint - 1) * 6..joint * 6]);
        let parent = PARENTS[joint].expect("only the pelvis has no parent");
        global[joint] = global[parent] * local;
    }
    global
}

/// The room-frame yaw of a joint's forward axis (its +Z) laid on the floor, in `body-rig`'s
/// convention: radians about +Y, zero facing -Z, positive turning toward -X.
pub fn room_yaw(rotation: DMat3) -> f64 {
    let forward = AMASS_FROM_ROOM.transpose() * rotation.z_axis;
    (-forward.x).atan2(-forward.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_amass_map_is_a_proper_rotation() {
        assert!((AMASS_FROM_ROOM.determinant() - 1.0).abs() < 1e-12);
        // The room's up is AMASS's up, and the room's forward is AMASS's +Y.
        assert_eq!(AMASS_FROM_ROOM * DVec3::Y, DVec3::Z);
        assert_eq!(AMASS_FROM_ROOM * DVec3::NEG_Z, DVec3::Y);
    }

    #[test]
    fn six_d_round_trips() {
        let rotation = DMat3::from_euler(glam::EulerRot::ZYX, 0.3, -1.1, 0.7);
        let six = to_6d(rotation).map(|v| v as f32);
        let back = from_6d(&six);
        assert!(
            (back - rotation)
                .to_cols_array()
                .iter()
                .all(|v| v.abs() < 1e-6)
        );
    }

    #[test]
    fn an_upright_body_facing_room_forward_has_zero_yaw() {
        // A joint whose forward is AMASS +Y (the room's -Z) and whose up is AMASS +Z.
        let upright = DMat3::from_cols(DVec3::NEG_X, DVec3::Z, DVec3::Y);
        assert!(room_yaw(upright).abs() < 1e-12);
        // Turned a quarter toward the room's -X (AMASS -X), the yaw is a positive quarter turn.
        let turned = DMat3::from_rotation_z(std::f64::consts::FRAC_PI_2) * upright;
        assert!((room_yaw(turned) - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }
}
