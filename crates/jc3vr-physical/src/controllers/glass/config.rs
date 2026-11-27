//! Breaking glass's settings, `controllers.glass`.

use jc3vr_core::config::ConfigSlot;
use serde::{Deserialize, Serialize};

/// The glass breaking's settings, at `controllers.glass` in the settings file; the payload binds
/// the slot at startup.
pub static GLASS_CONFIG: ConfigSlot<GlassConfig> = ConfigSlot::new("controllers.glass");

/// Breaking glass by hand (see [`crate::controllers::glass`]). It acts only under the controller
/// scheme, and its tuning is in [`GLASS`](crate::controllers::glass::tuning::GLASS).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlassConfig {
    /// Let bullets pass cleanly through a pane brought down, rather than meeting the glass's shapes
    /// that hiding it leaves ([`bullets`](crate::controllers::glass::bullets)).
    pub bullets_pass: bool,
}

impl GlassConfig {
    pub const fn new() -> Self {
        Self { bullets_pass: true }
    }
}

impl Default for GlassConfig {
    fn default() -> Self {
        Self::new()
    }
}
