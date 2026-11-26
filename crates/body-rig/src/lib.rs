//! The grounded body rig's pure core, shared by the payload, which runs it once per rendered frame
//! against the live tracking, and the offline evaluator in `tools/tracking-synth`, which runs it
//! over a recording. See `docs/mod/body/rig.md`.
//!
//! - [`pelvis`]: whether the head is out over Rico's pelvis because the player leans or because the
//!   player stepped, from one spine segment between the waist and the neck pivots.
//! - [`step`]: when a step commits, and the velocity that drives the pelvis back under the neck.
//! - [`yaw`]: the chest following the head within a clamp, and Rico's facing (the legs) turning
//!   only for a large or sustained turn of the head, or while he moves.
//! - [`estimate`]: the learned body estimator's estimate turned into a target for the legs and a
//!   pull on the chest.
//! - [`height`]: the standing neck height, measured rather than assumed.
//! - [`fk`]: the player's pelvis, hips, knees, and ankles by forward kinematics from the tracked
//!   neck and the estimate's rotations.
//! - [`contact`]: the IK body's feet, planted, swinging, or unsupported, and the rules that step
//!   them.
//! - [`brace`]: the IK body's feet braced on the face while the hands climb.
//! - [`ik_weight`]: the IK body's weight per state and channel, and its blend.
//! - [`rig`]: all of them advanced together.
//! - [`tuning`]: the tuning of all of them that the player's settings do not hold, which the body
//!   crate's `tuning/rig.toml` gives.
//!
//! [`angle`], [`rotation`], [`geometry`], and [`smooth`] hold the heading, rotation, geometry, and
//! smoothing primitives the payload shares.
//!
//! Everything works in the room frame: metres over the floor, +X right, +Y up, -Z forward, with
//! headings as yaws about +Y ([`angle`]).

pub mod angle;
pub mod body;
pub mod brace;
pub mod contact;
pub mod estimate;
pub mod fk;
pub mod geometry;
pub mod height;
pub mod ik_weight;
pub mod pelvis;
pub mod rig;
pub mod rotation;
pub mod smooth;
pub mod step;
pub mod tuning;
pub mod yaw;

pub use crate::{
    body::PlayerBody,
    estimate::{BodyEstimate, EstimateTuning},
    pelvis::{Bend, PelvisSolve, PelvisTuning},
    rig::{GroundedRig, RigFrame, RigOutput, RigTuning},
    step::{StepPhase, StepTuning, drive_velocity},
    tuning::CoreTuning,
    yaw::{TurnCause, YawTuning},
};
