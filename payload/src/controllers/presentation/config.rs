//! The presentation layer's settings.

use serde::{Deserialize, Serialize};

/// The render-rate presentation of the player's body (see
/// [`crate::controllers::presentation`]): the torso bend after the head and the arms solved again
/// to the hands. On by default: it only re-poses what the simulation drew a tick behind, and leaves
/// a pose the simulation agrees with as it is.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresentationConfig {
    /// Re-pose the upper body every rendered frame from the latest tracking. Off, the late update
    /// re-poses only the hands and the forearms, as before.
    pub enabled: bool,
}

impl PresentationConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for PresentationConfig {
    fn default() -> Self {
        Self::new()
    }
}
