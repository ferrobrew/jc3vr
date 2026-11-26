//! The collider view's settings.

use serde::{Deserialize, Serialize};

/// The collider view: the physics world's bodies near the player drawn in the world-space overlay,
/// each body's box and shape, static ones too, with the mod's joints, the contact flashes, the
/// player's ground, and the body under the gaze. A diagnostic, off by default.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColliderViewConfig {
    /// Draw the view.
    pub enabled: bool,
}

impl ColliderViewConfig {
    pub const fn new() -> Self {
        Self { enabled: false }
    }
}

impl Default for ColliderViewConfig {
    fn default() -> Self {
        Self::new()
    }
}
