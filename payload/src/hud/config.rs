//! Floating-HUD configuration types. See `docs/mod/hud.md`.

use serde::{Deserialize, Serialize};

/// Floating-HUD settings. See `docs/mod/hud.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HudConfig {
    /// Redirect the HUD into our own offscreen texture (the first step toward the floating panel).
    /// Off leaves the HUD on the engine surface as normal.
    pub redirect: bool,
    /// Draw the redirected HUD back into the scene as a floating quad, per eye. Requires `redirect`.
    pub quad: bool,
    /// Distance from the eye to the panel, in meters. The panel resizes with distance to keep a
    /// constant apparent (angular) size, so this can be changed freely without the HUD growing or
    /// shrinking. Comfort band: 1.5-3m.
    pub distance: f32,
    /// Apparent-size multiplier for the panel; `1.0` is the comfortable baseline (4 m wide at 3 m).
    /// The physical size is derived from this, [`distance`](HudConfig::distance), and the effective
    /// aspect (see [`crate::hud::panel_height`]), so changing the distance or aspect keeps the panel
    /// looking the same size and fitting the same horizontal content.
    pub panel_scale: f32,
    /// Dynamic panel distance from the scene. See [`crate::hud::depth`].
    pub depth_shift: DepthShiftConfig,
    /// The interactive egui debug panel floating in 3D space in VR (issue #24). Off by default;
    /// only takes effect while an OpenXR session is running.
    #[serde(default)]
    pub egui_panel: EguiPanelConfig,
    /// The world-marker prototype: the game's on-screen point-of-interest markers drawn as
    /// billboards at their world points instead of on the floating panel. See
    /// [`crate::hud::world_markers`].
    #[serde(default)]
    pub world_markers: WorldMarkersConfig,
    /// The HUD atlas: parts of the gameplay HUD moved into their own cells of the HUD texture for
    /// the capture and drawn on their own quads (the world markers, the ammunition readout at the
    /// gun hand, the tether readout on the forearm, and the heat readout on the wrist). See
    /// [`crate::hud::atlas`].
    #[serde(default)]
    pub atlas: AtlasConfig,
}
impl HudConfig {
    pub const fn new() -> Self {
        Self {
            redirect: true,
            quad: true,
            distance: 6.0,
            panel_scale: 0.8,
            depth_shift: DepthShiftConfig::new(),
            egui_panel: EguiPanelConfig::new(),
            world_markers: WorldMarkersConfig::new(),
            atlas: AtlasConfig::new(),
        }
    }
}

/// The world-marker prototype ([`crate::hud::world_markers`]): each on-screen point-of-interest
/// marker is cropped out of the HUD texture and drawn as a billboard at its world point, keeping
/// the angular size it has on the panel, and its rectangle is cut out of the panel.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldMarkersConfig {
    /// Master toggle.
    pub enabled: bool,
}

impl WorldMarkersConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for WorldMarkersConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The HUD atlas ([`crate::hud::atlas`]): while it is on, the gameplay HUD lays out inside the
/// stage, and at each capture the parts below are moved into cells in the bands above and below
/// the stage, then moved back. The world-placed markers move into cells too (with
/// [`WorldMarkersConfig::enabled`]); markers that find no cell are cropped from the panel.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AtlasConfig {
    /// Master toggle.
    pub enabled: bool,
    /// The ammunition counters the mod draws above each hand holding a gun (the magazine and the
    /// reserve), under the controller scheme, upright and facing the eye. The game's own weapon
    /// readout (`MCI_weapons_parent`) shows the wielded weapon alone; with the atlas on it keeps
    /// its cell, off the panel, and is not drawn, so both hands' guns read alike.
    pub ammo: AtlasPartConfig,
    /// The health bar (drawn by the mod from the player character's health), the heat readout
    /// (`MCI_heat_container`), and the grenade readout (`MCI_grenade_parent`), both moved into
    /// cells, on the inside of the off-hand's wrist, under the controller scheme. The controls
    /// summary sits on the back of the same wrist.
    pub wrist: AtlasPartConfig,
    /// The game's weapon selection cross (`MCI_weapon_selection_wheel`) drawn where the hand
    /// steering the weapon wheel was when the wheel opened, under the controller scheme, while the
    /// wheel shows it and for its hide animation after. Without the atlas, the wheel places the
    /// cross on the panel instead.
    pub cross: AtlasPartConfig,
    /// The game's tether readout (`MCI_dual_tether`: the count of tethers in use out of the most
    /// the grapple holds, and the retract state) drawn above the off-hand's forearm, where the
    /// grapple fires from, under the controller scheme, upright and facing the eye. The game shows
    /// it while tethers are attached and fades it out after; the quad carries that fade.
    pub tether: AtlasPartConfig,
}

impl AtlasConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            ammo: AtlasPartConfig::new(),
            wrist: AtlasPartConfig::new(),
            cross: AtlasPartConfig::new(),
            tether: AtlasPartConfig::new(),
        }
    }
}

impl Default for AtlasConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// One of the atlas's body-anchored readouts. Its placement is fixed in [`crate::hud::anchored`]
/// and its capture in [`crate::hud::atlas`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AtlasPartConfig {
    pub enabled: bool,
}

impl AtlasPartConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for AtlasPartConfig {
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
    /// Opening through the panel shortcut requests editing, and the preference defaults off.
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
    /// Whether the controllers point at the panel: either hand's aim ray moves the pointer, its
    /// trigger clicks, its upper face button right-clicks, and its thumbstick scrolls. The desktop
    /// mouse keeps working alongside; whichever was used last owns the pointer. On by default.
    pub controller_pointer: bool,
}

impl EguiPanelConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            aspect: 4.0 / 3.0,
            resolution: (1280, 960),
            show_on_mirror: false,
            controller_pointer: true,
        }
    }
}

impl Default for EguiPanelConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Dynamic panel distance (issue #14): the panel eases toward a near distance while the player
/// rides a vehicle, back to the base [`distance`](HudConfig::distance) otherwise, and always far
/// during full-screen UI. The tuning is fixed in [`crate::hud::depth`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DepthShiftConfig {
    /// Master toggle.
    pub enabled: bool,
}

impl DepthShiftConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for DepthShiftConfig {
    fn default() -> Self {
        Self::new()
    }
}
