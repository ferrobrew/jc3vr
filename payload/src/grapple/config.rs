//! Grapple comfort configuration. Lives on [`crate::headpose::config::HeadPoseConfig`] as
//! `headpose.grapple`, so it persists and live-edits with the rest of the headpose tunables.

use serde::{Deserialize, Serialize};

/// How the body frame the head composes onto is filtered while the grapple owns the character's
/// rotation.
///
/// The player aims the grapple by looking at the target, so the head already carries the rotation
/// toward it; the grapple then rotates the body toward the same target, and the `body × head`
/// composition applies that rotation twice — the view swings past whatever was being looked at
/// (issue #36).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum GrappleComfortMode {
    /// No filtering; the grapple swings the view with the body.
    Off,
    /// Hold the body frame from just before the fire, so the composed view stays world-stable
    /// from fire to landing — what the player is looking at stays looked-at, and only the HMD (or
    /// mouse) moves the view. Cancels the grapple's pitch *and* yaw double-count.
    #[default]
    HoldView,
    /// Flatten the body frame to its yaw, keeping the view level. The pitch double-count is
    /// removed, but the yaw toward the target still composes; kept as an option for players who
    /// prefer the view to follow the body's heading.
    LevelPitch,
}

/// Grapple comfort settings; see [`GrappleComfortMode`] and [`crate::grapple`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GrappleComfortConfig {
    /// The body-frame filter mode. Under [`GrappleComfortMode::HoldView`] on foot in VR, the yaw
    /// accumulated over the reel is handed to the body at the reel's end rather than to the view.
    pub mode: GrappleComfortMode,
}

impl GrappleComfortConfig {
    pub const fn new() -> Self {
        Self {
            mode: GrappleComfortMode::HoldView,
        }
    }
}

impl Default for GrappleComfortConfig {
    fn default() -> Self {
        Self::new()
    }
}
