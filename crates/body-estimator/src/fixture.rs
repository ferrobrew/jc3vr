//! The golden fixture the tests compare against: real windows from a recorded take, run through
//! the reference Python pipeline and the original PyTorch model (`tests/fixtures/`). It holds the
//! inputs and outputs only, never the weights.

use glam::{DMat3, DQuat, DVec3};
use safetensors::SafeTensors;

use crate::{
    features::{SparseFrame, WINDOW_POSES},
    smpl::JointPose,
    tracking::TrackedFrame,
};

pub(crate) struct Fixture {
    bytes: Vec<u8>,
}

impl Fixture {
    pub fn load() -> Fixture {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/egoposer_golden.safetensors"
        );
        Fixture {
            bytes: std::fs::read(path).unwrap_or_else(|e| panic!("the fixture at {path}: {e}")),
        }
    }

    fn file(&self) -> SafeTensors<'_> {
        SafeTensors::deserialize(&self.bytes).expect("the fixture is a safetensors file")
    }

    pub fn tensor(&self, name: &str) -> Vec<f32> {
        let file = self.file();
        let view = file
            .tensor(name)
            .unwrap_or_else(|_| panic!("the fixture has no `{name}`"));
        view.data()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    }

    pub fn has(&self, name: &str) -> bool {
        self.file().names().contains(&name)
    }

    pub fn windows(&self) -> usize {
        (0..)
            .take_while(|i| self.has(&format!("w{i}.features")))
            .count()
    }

    /// The field of view the fixture's masks were made with, degrees.
    pub fn fov_deg(&self) -> f64 {
        let (_, metadata) = SafeTensors::read_metadata(&self.bytes).expect("the fixture's header");
        metadata
            .metadata()
            .as_ref()
            .and_then(|m| m.get("fov_deg"))
            .and_then(|v| v.parse().ok())
            .expect("the fixture notes its field of view")
    }

    /// The window's room-frame tracking, one frame per pose.
    pub fn tracked_frames(&self, window: usize) -> Vec<TrackedFrame> {
        let get = |name: &str| self.tensor(&format!("w{window}.{name}"));
        let (eyes, head) = (get("eyes"), get("head_q"));
        let (left, left_q, right, right_q) = (
            get("grip_l"),
            get("grip_l_q"),
            get("grip_r"),
            get("grip_r_q"),
        );
        (0..WINDOW_POSES)
            .map(|i| TrackedFrame {
                eyes: vec3(&eyes[i * 3..]),
                head: quat(&head[i * 4..]),
                grips: [
                    (vec3(&left[i * 3..]), quat(&left_q[i * 4..])),
                    (vec3(&right[i * 3..]), quat(&right_q[i * 4..])),
                ],
            })
            .collect()
    }

    /// The window's SMPL head and wrist joints as the reference computed them.
    pub fn joint_frames(&self, window: usize) -> Vec<SparseFrame> {
        let rotations = self.tensor(&format!("w{window}.joint_rot"));
        let positions = self.tensor(&format!("w{window}.joint_pos"));
        (0..WINDOW_POSES)
            .map(|i| {
                let joint = |j: usize| {
                    let base = (i * 3 + j) * 9;
                    let row_major: [f64; 9] = std::array::from_fn(|k| rotations[base + k].into());
                    JointPose {
                        // The fixture is row-major; glam's arrays are column-major.
                        rotation: DMat3::from_cols_array(&row_major).transpose(),
                        position: vec3(&positions[(i * 3 + j) * 3..]),
                    }
                };
                SparseFrame {
                    head: joint(0),
                    left_wrist: joint(1),
                    right_wrist: joint(2),
                }
            })
            .collect()
    }

    pub fn joint_window(&self, window: usize) -> [SparseFrame; WINDOW_POSES] {
        self.joint_frames(window)
            .try_into()
            .unwrap_or_else(|_| panic!("the fixture's window {window} has {WINDOW_POSES} poses"))
    }
}

/// The largest absolute difference between two equally long sequences.
pub(crate) fn max_abs_diff<A: Copy + Into<f64>, B: Copy + Into<f64>>(a: &[A], b: &[B]) -> f64 {
    assert_eq!(a.len(), b.len(), "the compared tensors differ in length");
    a.iter()
        .zip(b)
        .map(|(&x, &y)| (x.into() - y.into()).abs())
        .fold(0.0, f64::max)
}

fn vec3(v: &[f32]) -> DVec3 {
    DVec3::new(v[0].into(), v[1].into(), v[2].into())
}

/// A quaternion stored `x, y, z, w`.
fn quat(v: &[f32]) -> DQuat {
    DQuat::from_xyzw(v[0].into(), v[1].into(), v[2].into(), v[3].into())
}
