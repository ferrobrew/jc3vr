//! The math the body rig, the input layer, and the layer crates above them share, so that each
//! convention and each degenerate case is settled once.
//!
//! - [`angle`]: headings, their wrap, and the one heading of a rotation ([`angle::heading`]).
//! - [`rotation`]: a rotation's level and twist, and rotations the short way round.
//! - [`geometry`]: segments, spheres, casts, and the velocity of a point on a rigid body.
//! - [`smooth`]: exponential follows, critically damped springs, and smoothstep.
//! - [`axis`]: an input's travel past its deadzone, shaped by a response exponent.
//! - [`two_bone`]: the middle joint of a two-bone limb.
//! - [`shoulders`]: the shoulder joints from the eyes and the torso's heading.
//! - [`side`]: the two sides of the body.
//! - [`pose`]: a rigid transform and its algebra.
//! - [`constants`]: gravity and the shared test fixtures.
//!
//! Positions use +X right, +Y up, and -Z forward, in metres.

pub mod angle;
pub mod axis;
pub mod constants;
pub mod geometry;
pub mod pose;
pub mod rotation;
pub mod shoulders;
pub mod side;
pub mod smooth;
pub mod two_bone;

pub use crate::math::{pose::Pose, side::Side};
