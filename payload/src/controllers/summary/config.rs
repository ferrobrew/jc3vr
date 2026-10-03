//! The controls summary's settings: the panel, when it shows, where it sits on the wrist, and the
//! suppression of the game's own button hints.

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// The controls summary and the game's own button hints under the controller scheme. Both act only
/// under the controller scheme, so both default on.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SummaryConfig {
    /// Show the controls summary panel on the off-hand wrist.
    pub enabled: bool,
    /// Hide the HUD's context button hints, which show gamepad glyphs for the actions the
    /// controllers drive with other inputs.
    pub hide_game_hints: bool,
    /// Show the panel while the player glances at the wrist.
    pub show_on_glance: bool,
    /// The glance's geometry and timing.
    pub glance: GlanceConfig,
    /// Show the panel for this long after the mode changes, seconds; zero never does.
    pub show_on_mode_change_s: f32,
    /// Show the panel while the off-hand's trackpad is touched (Index controllers only).
    pub show_on_trackpad_touch: bool,
    /// The panel's place on the wrist.
    pub placement: WristPlacement,
    /// The panel texture's size, pixels. Its aspect sets the panel's height from its width.
    pub resolution: (u32, u32),
    /// The fade in and out, seconds.
    pub fade_s: f32,
}

impl SummaryConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            hide_game_hints: true,
            show_on_glance: true,
            glance: GlanceConfig::new(),
            show_on_mode_change_s: 3.0,
            show_on_trackpad_touch: true,
            placement: WristPlacement::new(),
            resolution: (480, 640),
            fade_s: 0.15,
        }
    }
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// When a look at the wrist counts as a glance.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GlanceConfig {
    /// The largest angle between the head's forward and the direction to the panel, degrees.
    pub view_cone_deg: f32,
    /// The largest angle between the panel's face and the direction back to the head, degrees.
    pub facing_deg: f32,
    /// How much both angles widen once the panel is shown, degrees, so that it does not flicker at
    /// the edge.
    pub hysteresis_deg: f32,
    /// How long the look must hold before the panel shows, seconds, so that a hand sweeping past
    /// the view during a fight does not show it.
    pub dwell_s: f32,
    /// How long the panel stays after the look ends, seconds.
    pub linger_s: f32,
}

impl GlanceConfig {
    pub const fn new() -> Self {
        Self {
            view_cone_deg: 30.0,
            facing_deg: 45.0,
            hysteresis_deg: 10.0,
            dwell_s: 0.15,
            linger_s: 0.4,
        }
    }
}

impl Default for GlanceConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The panel's place on the off-hand.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WristPlacement {
    /// The panel's centre in the controller's grip frame, metres: -X out of the back of the hand,
    /// +Z back along the forearm.
    pub offset_m: Vec3,
    /// The panel's width, metres.
    pub width_m: f32,
}

impl WristPlacement {
    pub const fn new() -> Self {
        Self {
            offset_m: Vec3::new(-0.05, 0.0, 0.12),
            width_m: 0.12,
        }
    }
}

impl Default for WristPlacement {
    fn default() -> Self {
        Self::new()
    }
}
