//! The world-space overlay layer's diagnostic switches.

use jc3vr_core::config::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::intent::tuning::CONTROLS;

/// The world-space overlay layer's settings, at `controllers.overlay` in the settings file; the
/// payload binds the slot at startup.
pub static OVERLAY_CONFIG: ConfigSlot<OverlayConfig> = ConfigSlot::new("controllers.overlay");

/// How far ahead of Rico the body mirror stands, metres (the controls tuning's
/// `overlay.body_mirror_distance_m`).
pub fn body_mirror_distance_m() -> f32 {
    CONTROLS.get().overlay.body_mirror_distance_m
}

/// The world-space overlay layer's switches: the grip markers and the in-world diagnostics. The
/// layer acts only under the controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    /// Mark every grip target (the sockets and pouches on the body, the weapons, the vehicle
    /// controls, the objects and surfaces in reach), coloured by kind, the one each hand's squeeze
    /// would take highlighted ([`crate::controllers::grab::markers`]), and the armed grapple
    /// snatch's latch on its target.
    pub markers: bool,
    /// Draw Rico's solved skeleton reflected in a mirror ahead of him, with the targets it was
    /// posed toward, for checking the body's alignment ([`crate::controllers::body::mirror`]). A
    /// diagnostic, off by default.
    pub body_mirror: bool,
    /// Draw the grounded rig's neck pivot, waist pivot, and the spine between them, and its lean
    /// rings on the floor around the pelvis, coloured by the step phase (`docs/mod/body/rig.md`).
    pub body_rig: bool,
    /// Draw arrows on the floor under the eyes for the learned body estimate's pelvis and chest
    /// headings, in every mode the estimator runs in, with the grounded rig's legs and chest beside
    /// them while it runs (`docs/mod/body/body-estimator.md`).
    pub body_estimate: bool,
    /// Mark each gun docked on a socket of the body where its placement puts it, with a line along
    /// its barrel, green while its model is shown and red while hidden
    /// (`controllers::weapons::inventory::docked_view`).
    pub docked_guns: bool,
}

impl OverlayConfig {
    pub const fn new() -> Self {
        Self {
            markers: true,
            body_mirror: false,
            body_rig: true,
            body_estimate: false,
            docked_guns: true,
        }
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self::new()
    }
}
