//! Floating-HUD configuration types. See `docs/mod/hud.md`.

use jc3vr_core::config::engine::{ConfigSlot, Invalid, positive};
use serde::{Deserialize, Serialize};

/// The HUD settings, at `hud` in the settings file; the payload binds the slot at startup.
pub static HUD_CONFIG: ConfigSlot<HudConfig> = ConfigSlot::new("hud");

/// Floating-HUD settings. See `docs/mod/hud.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HudConfig {
    /// Redirect the HUD into our own offscreen texture (the first step toward the floating panel).
    /// Off leaves the HUD on the engine surface as normal.
    pub redirect: bool,
    /// Distance from the eye to the panel, in meters. The panel resizes with distance to keep a
    /// constant apparent (angular) size, so this can be changed freely without the HUD growing or
    /// shrinking. Comfort band: 1.5-3m.
    pub distance: f32,
    /// Apparent-size multiplier for the panel; `1.0` is the comfortable baseline (4 m wide at 3 m).
    /// The physical size is derived from this, [`distance`](HudConfig::distance), and the effective
    /// aspect (see [`crate::hud::panel_height`]), so changing the distance or aspect keeps the panel
    /// looking the same size and fitting the same horizontal content.
    pub panel_scale: f32,
    /// The interactive egui debug panel floating in 3D space in VR (issue #24). Off by default;
    /// only takes effect while an OpenXR session is running.
    #[serde(default)]
    pub egui_panel: EguiPanelConfig,
    /// The HUD atlas: parts of the gameplay HUD moved into their own cells of the HUD texture for
    /// the capture and drawn on their own quads (the world markers, the ammunition readout at the
    /// gun hand, the tether readout on the forearm, and the heat readout on the wrist). See
    /// [`crate::hud::atlas`].
    #[serde(default)]
    pub atlas: AtlasConfig,
}
/// The Direct3D 11 two-dimensional texture axis limit.
pub(crate) const MAX_TEXTURE_DIMENSION: u32 = 16384;

/// Check that a HUD texture axis is within [`MAX_TEXTURE_DIMENSION`].
pub(crate) fn dimension(name: &str, value: u32) -> Result<(), Invalid> {
    if !(1..=MAX_TEXTURE_DIMENSION).contains(&value) {
        return Err(Invalid(format!(
            "{name} must be between 1 and {MAX_TEXTURE_DIMENSION}, got {value}"
        )));
    }
    Ok(())
}

impl HudConfig {
    /// Check the panel geometry and surfaces loaded from settings, returning the first setting
    /// that is out of range. Paths are relative to `hud`.
    pub fn validate(&self) -> Result<(), Invalid> {
        positive("distance", self.distance, 1000.0)?;
        positive("panel_scale", self.panel_scale, 100.0)?;
        let e = &self.egui_panel;
        positive("egui_panel.aspect", e.aspect, 100.0)?;
        dimension("egui_panel.resolution[0]", e.resolution.0)?;
        dimension("egui_panel.resolution[1]", e.resolution.1)
    }

    pub const fn new() -> Self {
        Self {
            redirect: true,
            distance: 6.0,
            panel_scale: 0.8,
            egui_panel: EguiPanelConfig::new(),
            atlas: AtlasConfig::new(),
        }
    }
}

impl Default for HudConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The HUD atlas ([`crate::hud::atlas`]): while it is on, the gameplay HUD lays out inside the
/// stage, and at each capture its body-anchored readouts are moved into cells in the bands above
/// and below the stage, then moved back: the ammunition counters above each gun hand, the health,
/// heat, and grenade readouts on the off-hand's wrist, the weapon selection cross at the hand
/// steering the weapon wheel, and the tether readout above the off-hand's forearm, each under the
/// controller scheme. The world-placed markers move into cells too; markers that find no cell are
/// cropped from the panel. The placements are fixed in [`crate::hud::anchored`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AtlasConfig {
    /// Master toggle.
    pub enabled: bool,
}

impl AtlasConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for AtlasConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The interactive egui debug panel rendered as a 2D surface floating in 3D space in VR (issue #24).
///
/// The panel is an independent floating surface: it has its own offscreen render target and its own
/// placement in the player's tracked space, so it never perturbs the gameplay HUD. It only takes
/// effect while an OpenXR session is running; on the flat desktop the egui debug overlay stays on the
/// back buffer as before. Its placement and click tuning are fixed in
/// [`crate::hud::egui_panel`] and [`crate::hud::pointer`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EguiPanelConfig {
    /// Panel visibility. While a session runs, the debug UI renders into the floating surface.
    /// Input ownership is separate: Allow gameplay input keeps the panel visible and read-only.
    /// Opening through the panel shortcut requests editing, and the preference defaults off. Not
    /// saved, so a session that left the panel up does not open it at the next launch.
    #[serde(skip)]
    pub enabled: bool,
    /// Aspect ratio (width / height) of the panel quad. Kept in step with
    /// [`resolution`](EguiPanelConfig::resolution) so the egui content is not stretched.
    pub aspect: f32,
    /// The panel texture's pixel resolution. egui lays out at this size, so it also sets the effective
    /// DPI of the panel content.
    pub resolution: (u32, u32),
    /// Whether to also composite the panel onto the desktop mirror while a VR session runs. Off by
    /// default: the panel is already visible in the headset, so the desktop mirror shows the
    /// unobstructed scene.
    pub show_on_mirror: bool,
}

impl EguiPanelConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            aspect: 4.0 / 3.0,
            resolution: (1280, 960),
            show_on_mirror: false,
        }
    }
}

impl Default for EguiPanelConfig {
    fn default() -> Self {
        Self::new()
    }
}
