//! The body rig: who moves the character's root, the chase that keeps it under the player's head,
//! and the lean the chase leaves alone (`docs/mod/body/rig.md`).
//!
//! - [`root`]: the root's owner per simulation tick, the chase, its ledger, and the cockpit shift;
//! - [`lean`]: how far the neck pivot may be out over the feet without a step;
//! - [`config`]: the settings, under `controllers.body.rig`.

pub mod config;
pub mod lean;
pub mod root;

#[cfg(test)]
pub(crate) mod harness;

pub use crate::controllers::body::rig::{
    config::RigConfig,
    root::{feet, lean_allowance},
};
