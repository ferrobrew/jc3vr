//! Camera hook configuration.

use serde::{Deserialize, Serialize};

pub(crate) const SCENE_NEAR_CLIP_M: f32 = 0.02;

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
