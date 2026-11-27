//! The grab view's settings.

use serde::{Deserialize, Serialize};

/// The grab view: what the grip and the palms' reach saw, drawn in the world-space overlay around
/// each hand: the reach rays, the lines to the grip targets, the refused grabs, the candidates'
/// boxes, and a held gun's grab zones. A diagnostic, on by default.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrabViewConfig {
    /// Draw the view.
    pub enabled: bool,
}

impl GrabViewConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for GrabViewConfig {
    fn default() -> Self {
        Self::new()
    }
}
