//! Camera hook configuration.

use serde::{Deserialize, Serialize};

/// VR head/body camera settings (was `hooks::camera::CameraSettings`).
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    pub enabled: bool,
    pub body_offset: glam::Vec3,
    pub head_offset: glam::Vec3,
    pub use_eye_matrices: bool,
    pub blurs_enabled: bool,
    pub always_use_t1: bool,
    /// Hide the player's head by collapsing its facial bones' skinning matrices in non-shadow
    /// passes (see `hooks::graphics_engine::render_block`): the whole head — face, eyes, hair,
    /// and any gear weighted to facial bones — contracts to a point inside the collar, while the
    /// shadow passes see the real palette, so the shadow keeps its head.
    pub hide_head_draws: bool,
    /// The scale head-hide: scale the HEAD bone and a facial-bone list to 0.001. A fallback for
    /// `hide_head_draws`, which is preferred because the scale approach also removes the head from
    /// the shadow, and its unscaled child bones leak the eyes into view.
    pub hide_head_scale: bool,
}
impl CameraConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            // Both offsets default to zero because the head is hidden: with
            // use_eye_matrices on (the default), the camera arm is the measured neck-to-eye arm
            // from the animated eye bones and head_offset is a correction on top of it; with it
            // off, head_offset is the whole arm from the neck pivot.
            body_offset: glam::Vec3::ZERO,
            head_offset: glam::Vec3::ZERO,
            use_eye_matrices: true,
            blurs_enabled: false,
            always_use_t1: false,
            hide_head_draws: true,
            hide_head_scale: false,
        }
    }
}
