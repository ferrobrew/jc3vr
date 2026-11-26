//! Camera hook configuration.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

pub const SCENE_NEAR_CLIP_M: f32 = 0.02;

/// The camera settings, at `camera` in the settings file; the payload binds the slot at startup.
pub static CAMERA_CONFIG: ConfigSlot<CameraConfig> = ConfigSlot::new("camera");

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

impl Default for CameraConfig {
    fn default() -> Self {
        Self::new()
    }
}
