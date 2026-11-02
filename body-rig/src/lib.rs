//! The grounded body rig's pure core, shared by the payload, which runs it once per rendered frame
//! against the live tracking, and the offline evaluator in `tools/tracking-synth`, which runs it
//! over a recording. See `docs/mod/body/rig.md`.
//!
//! - [`pelvis`]: whether the head is out over Rico's pelvis because the player leans or because the
//!   player stepped, from one spine segment between the waist and the neck pivots.
//! - [`step`]: when a step commits, and the velocity that drives the pelvis back under the neck.
//! - [`yaw`]: Rico's facing following the head through a deadzone, biased by the hands.
//! - [`height`]: the standing neck height, measured rather than assumed.
//! - [`rig`]: all of them advanced together.
//!
//! Everything works in the room frame: metres over the floor, +X right, +Y up, -Z forward, with
//! headings as yaws about +Y ([`angle`]).

pub mod angle;
pub mod body;
pub mod height;
pub mod pelvis;
pub mod rig;
pub mod step;
pub mod yaw;

pub use crate::{
    body::PlayerBody,
    pelvis::{Bend, PelvisSolve, PelvisTuning},
    rig::{GroundedRig, RigFrame, RigOutput, RigTuning},
    step::{StepPhase, StepTuning, drive_velocity},
    yaw::YawTuning,
};
