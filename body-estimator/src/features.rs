//! EgoPoser's sparse input: the head and both wrists over a window, as its AMASS data pipeline
//! (`prepare_data.py`) builds them, and which hands the headset could see.
//!
//! Each of a window's [`WINDOW_FRAMES`] frames has 54 values, for the head, the left wrist, and the
//! right wrist in that order within each block:
//!
//! - 0..18: each joint's global rotation in 6D ([`smpl::to_6d`](crate::smpl::to_6d));
//! - 18..36: each joint's rotation since the previous frame, `R_prev^T R`, in 6D;
//! - 36..45: each joint's position in AMASS's world;
//! - 45..54: each joint's position less the previous frame's.
//!
//! The rates are per frame at 60 Hz, the rate the network was trained at, so a window spans 80
//! frames and is built from [`WINDOW_POSES`] poses, the first only for the rates.

use glam::DVec3;

use crate::smpl::{JointPose, to_6d};

/// The frames in one window of the network's input.
pub const WINDOW_FRAMES: usize = 80;

/// The poses a window is built from: one more than its frames, for the first frame's rates.
pub const WINDOW_POSES: usize = WINDOW_FRAMES + 1;

/// The values in one frame of the sparse input.
pub const FEATURES: usize = 54;

/// The SMPL head and wrist joints of one frame, in AMASS's world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SparseFrame {
    pub head: JointPose,
    pub left_wrist: JointPose,
    pub right_wrist: JointPose,
}

impl SparseFrame {
    /// The joints in the sparse input's order: head, left wrist, right wrist.
    pub fn joints(&self) -> [JointPose; 3] {
        [self.head, self.left_wrist, self.right_wrist]
    }
}

/// Whether each hand was visible to the headset in each frame of a window. EgoPoser zeroes a hand's
/// inputs in the frames it could not be seen; tracked controllers are always visible.
#[derive(Debug, Clone, PartialEq)]
pub struct HandVisibility {
    pub left: [bool; WINDOW_FRAMES],
    pub right: [bool; WINDOW_FRAMES],
}

impl HandVisibility {
    /// Both hands visible throughout, as for tracked controllers.
    pub const ALWAYS: HandVisibility = HandVisibility {
        left: [true; WINDOW_FRAMES],
        right: [true; WINDOW_FRAMES],
    };

    /// Each hand visible where it lies within a field of view `horizontal_deg` wide and
    /// `vertical_deg` tall about the head's forward, as EgoPoser simulates a headset's hand
    /// tracking. The visibility of frame `i` is that of `poses[i + 1]`, the pose the frame is of.
    pub fn from_field_of_view(
        poses: &[SparseFrame; WINDOW_POSES],
        horizontal_deg: f64,
        vertical_deg: f64,
    ) -> HandVisibility {
        let mut visibility = HandVisibility::ALWAYS;
        for (i, pose) in poses[1..].iter().enumerate() {
            let fov = |hand: &JointPose| {
                in_field_of_view(&pose.head, hand.position, horizontal_deg, vertical_deg)
            };
            visibility.left[i] = fov(&pose.left_wrist);
            visibility.right[i] = fov(&pose.right_wrist);
        }
        visibility
    }
}

/// One window of the network's input: the sparse features and the hands' visibility.
#[derive(Debug, Clone, PartialEq)]
pub struct SparseWindow {
    pub features: [[f32; FEATURES]; WINDOW_FRAMES],
    pub visibility: HandVisibility,
}

impl SparseWindow {
    /// The window of `poses`, the oldest first, with both hands visible.
    pub fn from_poses(poses: &[SparseFrame; WINDOW_POSES]) -> SparseWindow {
        let mut features = [[0.0f32; FEATURES]; WINDOW_FRAMES];
        for (row, pair) in features.iter_mut().zip(poses.windows(2)) {
            *row = frame_features(&pair[0], &pair[1]);
        }
        SparseWindow {
            features,
            visibility: HandVisibility::ALWAYS,
        }
    }

    /// The window with `visibility` in place of its own.
    pub fn with_visibility(self, visibility: HandVisibility) -> SparseWindow {
        SparseWindow { visibility, ..self }
    }
}

/// EgoPoser's `in_fov`: whether a point lies within the field of view about the head joint's
/// forward (+Z in the SMPL head frame), judged separately in the horizontal and vertical planes.
/// A point straight above or beside the head, where an angle is undefined, is outside.
pub fn in_field_of_view(
    head: &JointPose,
    point: DVec3,
    horizontal_deg: f64,
    vertical_deg: f64,
) -> bool {
    let local = head.rotation.transpose() * (point - head.position);
    let horizontal = (local.z / (local.x * local.x + local.z * local.z).sqrt()).acos();
    let vertical = (local.z / (local.y * local.y + local.z * local.z).sqrt()).acos();
    horizontal <= (horizontal_deg / 2.0).to_radians()
        && vertical <= (vertical_deg / 2.0).to_radians()
}

/// One frame's 54 values from its pose and the previous one. Computed in f64, as the reference
/// pipeline does, and narrowed for the network.
fn frame_features(previous: &SparseFrame, current: &SparseFrame) -> [f32; FEATURES] {
    let mut row = [0.0f32; FEATURES];
    let joints = current.joints();
    let previous = previous.joints();
    for (j, (now, before)) in joints.iter().zip(&previous).enumerate() {
        let rotation = to_6d(now.rotation);
        let rate = to_6d(before.rotation.transpose() * now.rotation);
        let step = now.position - before.position;
        for k in 0..6 {
            row[j * 6 + k] = rotation[k] as f32;
            row[18 + j * 6 + k] = rate[k] as f32;
        }
        for k in 0..3 {
            row[36 + j * 3 + k] = now.position[k] as f32;
            row[45 + j * 3 + k] = step[k] as f32;
        }
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        fixture::{Fixture, max_abs_diff},
        tracking::GripOffset,
    };

    #[test]
    fn features_match_the_reference_pipeline() {
        let fixture = Fixture::load();
        for window in 0..fixture.windows() {
            let expected = fixture.tensor(&format!("w{window}.features"));
            // From the reference joints...
            let poses = fixture.joint_window(window);
            let ours = SparseWindow::from_poses(&poses);
            let from_joints = max_abs_diff(ours.features.as_flattened(), &expected);
            // ...and end to end from the room frame's tracking.
            let tracked = fixture.tracked_frames(window);
            let poses: Vec<SparseFrame> = tracked
                .iter()
                .map(|frame| SparseFrame::from_tracking(frame, &GripOffset::INDEX_BEST_GUESS))
                .collect();
            let poses: &[SparseFrame; WINDOW_POSES] = poses.as_slice().try_into().unwrap();
            let end_to_end = max_abs_diff(
                SparseWindow::from_poses(poses).features.as_flattened(),
                &expected,
            );
            eprintln!(
                "window {window}: features max abs error {from_joints:.2e} from joints, {end_to_end:.2e} from tracking"
            );
            assert!(from_joints < 1e-5 && end_to_end < 1e-5);
        }
    }

    #[test]
    fn field_of_view_matches_the_reference_masks() {
        let fixture = Fixture::load();
        let fov = fixture.fov_deg();
        let visibility = HandVisibility::from_field_of_view(&fixture.joint_window(0), fov, fov);
        let left = fixture.tensor("w0.fov_l");
        let right = fixture.tensor("w0.fov_r");
        for i in 0..WINDOW_FRAMES {
            assert_eq!(visibility.left[i], left[i] > 0.5, "left hand, frame {i}");
            assert_eq!(visibility.right[i], right[i] > 0.5, "right hand, frame {i}");
        }
        // The fixture's window has the left hand leave the view, so the test exercises both cases.
        assert!(visibility.left.iter().any(|v| !v) && visibility.left.iter().any(|v| *v));
    }
}
