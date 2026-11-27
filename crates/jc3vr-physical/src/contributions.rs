//! What the physical crate reads from the crates above it: one [`PhysicalContributions`] record,
//! registered once at install by the crate that provides it (`docs/architecture.md`, "Crate
//! layering").
//!
//! The record has one field per contribution, so a registrant that leaves one out fails to compile;
//! whether the record was registered at all is reported to the install's registration check through
//! [`registered`].

use std::sync::OnceLock;

/// The contributions the hand bodies run every step.
pub struct PhysicalContributions {
    /// The vehicle rail's part of the hand bodies' pre-simulation step, which carries the player
    /// along the skid. It runs after the drawn body's step and before the hand step, so a gripping
    /// hand's carry is the one the proxy keeps. Called on the thread that steps the physics world,
    /// outside the step, after the characters' proxy updates.
    pub rail_step: unsafe fn(),
}

/// Register the contributions. One registration: a second is refused and logged.
pub fn register(contributions: PhysicalContributions) {
    if CONTRIBUTIONS.set(contributions).is_err() {
        tracing::error!("physical: the contributions are already registered; keeping the first");
    }
}

/// Whether the contributions are registered.
pub fn registered() -> bool {
    CONTRIBUTIONS.get().is_some()
}

/// The registered contributions, or `None` before the install (and in the crate's own tests).
pub(crate) fn get() -> Option<&'static PhysicalContributions> {
    CONTRIBUTIONS.get()
}

static CONTRIBUTIONS: OnceLock<PhysicalContributions> = OnceLock::new();
