//! From the room frame's tracked poses to the SMPL joints the estimators take.
//!
//! The room frame is OpenXR's (+X right, +Y up, -Z forward) with the floor at zero. AMASS's head
//! is the SMPL head joint, at the base of the skull, and its hands are the wrist joints, so the
//! tracked eye midpoint and grip poses are moved onto them by fixed offsets:
//!
//! - The head joint sits [`EYES_TO_HEAD_JOINT`] from the eye midpoint in the head's frame, as
//!   measured on the SMPL+H male template.
//! - The wrist sits at a [`GripOffset`] from the controller's grip pose, in the hand-tracking wrist
//!   convention (-Z toward the fingers, +Y out of the back of the hand, X = Y × Z), then turned
//!   into the SMPL wrist frame.

use glam::{DMat3, DQuat, DVec3, EulerRot};

use crate::{
    features::SparseFrame,
    smpl::{AMASS_FROM_ROOM, JointPose},
};

/// The SMPL head joint from the eye midpoint, in the head's frame: 9.1 cm below and 6.5 cm behind.
pub const EYES_TO_HEAD_JOINT: DVec3 = DVec3::new(0.0, -0.091, 0.065);

/// One frame of tracking in the room frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TrackedFrame {
    /// The midpoint of the eyes.
    pub eyes: DVec3,
    /// The head's orientation.
    pub head: DQuat,
    /// Each hand's OpenXR grip pose, left then right.
    pub grips: [(DVec3, DQuat); 2],
}

/// Where the wrist joint sits in the right hand's grip frame; the left hand's is its mirror image
/// across the body's sagittal plane.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GripOffset {
    /// The wrist's position in the right grip frame, metres.
    pub position_m: DVec3,
    /// The hand's rotation in the right grip frame, about the grip's X, then Y, then Z, degrees.
    pub rotation_deg: DVec3,
}

impl GripOffset {
    /// The payload's best guess for the Valve Index controller, the same as its
    /// `GripOffset::INDEX_BEST_GUESS`: the wrist a few centimetres behind and to the back-of-hand
    /// side of the grip centre, and the handle tilted toward the fingers.
    pub const INDEX_BEST_GUESS: GripOffset = GripOffset {
        position_m: DVec3::new(0.02, 0.075, 0.0),
        rotation_deg: DVec3::new(15.0, 0.0, 0.0),
    };
}

impl SparseFrame {
    /// The SMPL head and wrist joints of one frame of tracking.
    pub fn from_tracking(frame: &TrackedFrame, offset: &GripOffset) -> SparseFrame {
        let head = DMat3::from_quat(frame.head);
        let head_joint = JointPose {
            rotation: AMASS_FROM_ROOM * head * SMPL_FROM_OPENXR_HEAD,
            position: AMASS_FROM_ROOM * (frame.eyes + head * EYES_TO_HEAD_JOINT),
        };
        let wrist = |side: Side| {
            let (position, orientation) = frame.grips[side as usize];
            let grip = DMat3::from_quat(orientation);
            let (wrist_rotation, wrist_position) = grip_to_wrist(side, offset);
            JointPose {
                rotation: AMASS_FROM_ROOM * grip * wrist_rotation * side.smpl_from_wrist(),
                position: AMASS_FROM_ROOM * (position + grip * wrist_position),
            }
        };
        SparseFrame {
            head: head_joint,
            left_wrist: wrist(Side::Left),
            right_wrist: wrist(Side::Right),
        }
    }
}

/// The SMPL head frame from OpenXR's head frame: half a turn about +Y, since SMPL's +X is the
/// body's left and +Z its forward, where OpenXR's +X is right and -Z forward.
const SMPL_FROM_OPENXR_HEAD: DMat3 = DMat3::from_cols(
    DVec3::new(-1.0, 0.0, 0.0),
    DVec3::new(0.0, 1.0, 0.0),
    DVec3::new(0.0, 0.0, -1.0),
);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Side {
    Left = 0,
    Right = 1,
}

impl Side {
    /// The SMPL wrist frame (identity in the T-pose: +X the body's left, +Y up, +Z forward) in the
    /// hand-tracking wrist frame: its columns are the SMPL axes in wrist coordinates. The left
    /// hand's fingers point along SMPL +X in the T-pose and the right hand's along -X; the back of
    /// either hand is up and the thumb forward.
    fn smpl_from_wrist(self) -> DMat3 {
        match self {
            // x = -Z_w, y = Y_w, z = X_w.
            Side::Left => DMat3::from_cols(DVec3::NEG_Z, DVec3::Y, DVec3::X),
            // x = Z_w, y = Y_w, z = -X_w.
            Side::Right => DMat3::from_cols(DVec3::Z, DVec3::Y, DVec3::NEG_X),
        }
    }
}

/// The wrist frame in the grip frame, as the payload's `grip_to_wrist` composes it: the offset's
/// rotation times the right wrist's axes in the grip frame, mirrored in X for the left hand.
fn grip_to_wrist(side: Side, offset: &GripOffset) -> (DMat3, DVec3) {
    let degrees = offset.rotation_deg;
    let adjust = DMat3::from_euler(
        EulerRot::ZYX,
        degrees.z.to_radians(),
        degrees.y.to_radians(),
        degrees.x.to_radians(),
    );
    // The right wrist's X is the grip's Z, its Y the grip's X, and its Z the grip's Y.
    let right_wrist_in_grip = DMat3::from_cols(DVec3::Z, DVec3::X, DVec3::Y);
    let rotation = adjust * right_wrist_in_grip;
    match side {
        Side::Right => (rotation, offset.position_m),
        Side::Left => {
            let flip_x = DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0));
            (flip_x * rotation * flip_x, flip_x * offset.position_m)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{Fixture, max_abs_diff};

    #[test]
    fn tracking_converts_to_the_reference_joints() {
        let fixture = Fixture::load();
        for window in 0..fixture.windows() {
            let tracked = fixture.tracked_frames(window);
            let expected = fixture.joint_frames(window);
            let mut worst = 0.0f64;
            for (frame, reference) in tracked.iter().zip(&expected) {
                let ours = SparseFrame::from_tracking(frame, &GripOffset::INDEX_BEST_GUESS);
                for (a, b) in ours.joints().iter().zip(reference.joints().iter()) {
                    worst = worst.max(max_abs_diff(
                        &a.rotation.to_cols_array(),
                        &b.rotation.to_cols_array(),
                    ));
                    worst = worst.max(max_abs_diff(&a.position.to_array(), &b.position.to_array()));
                }
            }
            eprintln!("window {window}: tracking -> joints, max abs error {worst:.2e}");
            // The fixture holds f32 values of an f64 computation.
            assert!(worst < 1e-5, "window {window}: max abs error {worst}");
        }
    }
}
