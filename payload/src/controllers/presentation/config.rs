//! The presentation layer's settings.

use serde::{Deserialize, Serialize};

/// The render-rate presentation of the player's body (see
/// [`crate::controllers::presentation`]). The parts default on: each only re-poses what the
/// simulation drew a tick behind, and leaves a pose the simulation agrees with as it is.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresentationConfig {
    /// Re-pose the upper body every rendered frame from the latest tracking. Off, the late update
    /// re-poses only the hands and the forearms, as before.
    pub enabled: bool,
    /// Bend the spine and turn the head after the headset's motion since the tick.
    pub torso: bool,
    /// Solve the arms again from the shoulders to the render-time hands.
    pub arms: bool,
}

impl PresentationConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            torso: true,
            arms: true,
        }
    }
}

impl Default for PresentationConfig {
    fn default() -> Self {
        Self::new()
    }
}
