//! Camera hook configuration.

use serde::{Deserialize, Serialize};

/// VR head/body camera settings.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    /// Place the game's camera at the player's eyes.
    pub enabled: bool,
}
impl CameraConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}
