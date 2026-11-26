//! EgoPoser (Jiang et al., ECCV 2024): an SMPL body from an 80-frame window of head and hand
//! poses, by a slow-fast transformer encoder.
//!
//! [`EgoPoser::load`] reads the weights (the `weights` module describes the file), and
//! [`EgoPoser::infer`] runs one window ([`SparseWindow`]) through the network (the `network` module).
//! [`EgoPoser::infer_traced`] also returns the intermediate tensors, which the golden tests and
//! any other implementation of the network check themselves against. On Windows, the `gpu` module
//! runs the same network through Direct3D 11 compute shaders.

#[cfg(all(windows, feature = "d3d11"))]
pub mod gpu;
mod network;
mod weights;

use std::path::Path;

use glam::DMat3;

pub use crate::egoposer::weights::{WeightsError, WeightsErrorKind};
use crate::{
    egoposer::{network::forward, weights::Weights},
    features::SparseWindow,
    smpl::{JOINT_COUNT, PELVIS, SPINE3, global_rotations, room_yaw},
};

/// The loaded network.
#[derive(Debug, Clone, PartialEq)]
pub struct EgoPoser {
    weights: Weights,
}

/// One window's estimate: the body at the window's last frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EgoPoserOutput {
    /// The pelvis's global rotation in AMASS's world, 6D.
    pub root_orient: [f32; 6],
    /// The 21 other joints' rotations relative to their parents, 6D each, in SMPL joint order.
    pub pose_body: [f32; 126],
    /// The body-shape coefficients, from a checkpoint trained to estimate them.
    pub betas: Option<[f32; 16]>,
}

/// The tensors between the stages of one pass.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Trace {
    /// The network's input after the normalisation, the deltas, and the masks: 80 frames of 60.
    pub normalized: Vec<f32>,
    /// The summed slow and fast embeddings: 40 tokens of the model width.
    pub embedded: Vec<f32>,
    /// The tokens after each encoder layer. The last layer computes the last token alone, the one
    /// the decoders read, so its entry holds that one token.
    pub layers: Vec<Vec<f32>>,
}

impl EgoPoser {
    /// The network with the weights in the safetensors file at `path`.
    pub fn load(path: impl AsRef<Path>) -> Result<EgoPoser, WeightsError> {
        Ok(EgoPoser {
            weights: Weights::from_file(path.as_ref())?,
        })
    }

    /// The network with the weights in `bytes`, a safetensors file's contents.
    pub fn from_bytes(bytes: &[u8]) -> Result<EgoPoser, WeightsError> {
        Ok(EgoPoser {
            weights: Weights::from_bytes(bytes)?,
        })
    }

    /// Whether the checkpoint estimates the body's shape.
    pub fn estimates_shape(&self) -> bool {
        self.weights.betas.is_some()
    }

    pub fn infer(&self, window: &SparseWindow) -> EgoPoserOutput {
        forward(&self.weights, window, None)
    }

    pub fn infer_traced(&self, window: &SparseWindow) -> (EgoPoserOutput, Trace) {
        let mut trace = Trace::default();
        let output = forward(&self.weights, window, Some(&mut trace));
        (output, trace)
    }
}

impl EgoPoserOutput {
    /// Every joint's global rotation in AMASS's world.
    pub fn global_rotations(&self) -> [DMat3; JOINT_COUNT] {
        global_rotations(&self.root_orient, &self.pose_body)
    }

    /// The pelvis's heading in the room frame, in `body-rig`'s yaw convention.
    pub fn pelvis_yaw(&self) -> f64 {
        room_yaw(self.global_rotations()[PELVIS])
    }

    /// The upper chest's (spine3's) heading in the room frame, in `body-rig`'s yaw convention.
    pub fn chest_yaw(&self) -> f64 {
        room_yaw(self.global_rotations()[SPINE3])
    }
}

#[cfg(test)]
mod tests;
