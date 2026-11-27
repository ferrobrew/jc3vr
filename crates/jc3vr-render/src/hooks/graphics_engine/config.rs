//! Graphics-engine area configuration: exposure and post-effect toggles.

use jc3vr_core::config::ConfigSlot;
use serde::{Deserialize, Serialize};

/// The exposure settings, at `exposure` in the settings file; the payload binds the slot at
/// startup.
pub static EXPOSURE_CONFIG: ConfigSlot<ExposureConfig> = ConfigSlot::new("exposure");

/// The exposure pin.
#[derive(Clone, Serialize, Deserialize)]
pub struct ExposureConfig {
    /// Pin the applied exposure (`m_ExposureBrightPoint`) to `forced_value` instead of the engine's auto-exposure (A/B aid).
    pub force: bool,
    /// The pinned exposure value, used when `force` is set.
    pub forced_value: f32,
}
impl ExposureConfig {
    pub const fn new() -> Self {
        Self {
            force: false,
            forced_value: 0.11,
        }
    }
}

impl Default for ExposureConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The post-processing settings, at `post_fx` in the settings file; the payload binds the slot at
/// startup.
pub static POST_FX_CONFIG: ConfigSlot<PostFxConfig> = ConfigSlot::new("post_fx");

/// Post-effect skip toggles (bisection aids / VR cleanups).
#[derive(Clone, Serialize, Deserialize)]
pub struct PostFxConfig {
    pub skip_motion_blur: bool,
    pub skip_motion_blur_recon: bool,
    pub skip_dof: bool,
    pub dof_no_reproject: bool,
    pub skip_fade: bool,
    pub skip_glare: bool,
    pub skip_player_damage: bool,
    pub skip_sun_halo: bool,
    pub skip_histogram: bool,
    /// Drop the engine's anti-aliasing to none whenever FSR is not resolving in its place, so the
    /// image has no anti-aliasing at all (a baseline for comparing FSR and SMAA against).
    #[serde(default)]
    pub skip_anti_aliasing: bool,
}
impl PostFxConfig {
    pub const fn new() -> Self {
        Self {
            skip_motion_blur: false,
            skip_motion_blur_recon: false,
            skip_dof: false,
            dof_no_reproject: true,
            skip_fade: false,
            skip_glare: false,
            skip_player_damage: false,
            skip_sun_halo: false,
            skip_histogram: false,
            skip_anti_aliasing: false,
        }
    }
}

impl Default for PostFxConfig {
    fn default() -> Self {
        Self::new()
    }
}
