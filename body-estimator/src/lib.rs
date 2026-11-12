//! A learned full-body estimator's inference, in plain Rust: EgoPoser (Jiang et al., ECCV 2024),
//! which estimates an SMPL body from a window of head and hand poses.
//!
//! - [`tracking`]: from the room frame's tracked head and grip poses to the SMPL head and wrist
//!   joints in AMASS's z-up world, the convention the network was trained in.
//! - [`features`]: the 54-D sparse input EgoPoser's data pipeline builds from those joints over an
//!   80-frame window, and the hands' field-of-view masks.
//! - [`egoposer`]: the weights and the forward pass, including the normalisation the network does
//!   on its input; on Windows, with the `d3d11` feature (on by default), also the forward pass as
//!   Direct3D 11 compute shaders (`egoposer::gpu`).
//! - [`smpl`]: the SMPL kinematic tree, the 6D rotation format, and the yaws read off the output.
//!
//! The CPU pass is the reference implementation: clear rather than fast, and checked against the
//! original PyTorch model's outputs by golden tests (`tests/fixtures/egoposer_golden.safetensors`);
//! the GPU pass is tested against it. The weights are not in the repository; the tests that need
//! them read the path from `JC3VR_EGOPOSER_WEIGHTS` and skip without it.

pub mod egoposer;
pub mod features;
pub mod smpl;
pub mod tracking;

mod ops;

pub use crate::{
    egoposer::{EgoPoser, EgoPoserOutput, WeightsError, WeightsErrorKind},
    features::{HandVisibility, SparseFrame, SparseWindow, WINDOW_FRAMES, WINDOW_POSES},
    smpl::JointPose,
    tracking::{GripOffset, TrackedFrame},
};

#[cfg(test)]
mod fixture;
