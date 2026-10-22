//! Floating-HUD configuration types. See `docs/mod/hud.md`.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::controllers::summary::config::GlanceConfig;

/// Floating-HUD settings. See `docs/mod/hud.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HudConfig {
    /// Redirect the HUD into our own offscreen texture (the first step toward the floating panel).
    /// Off leaves the HUD on the engine surface as normal.
    pub redirect: bool,
    /// Draw the redirected HUD back into the scene as a floating quad, per eye. Requires `redirect`.
    pub quad: bool,
    /// Aspect ratio (width / height) for the gameplay HUD; `1.0` is square. The effective aspect for
    /// the current frame ([`hud_aspect`](HudConfig::hud_aspect) or [`movie_aspect`](HudConfig::movie_aspect),
    /// per the [`HudMode`](crate::hud::HudMode)) is the single source of truth for the HUD's shape:
    /// the render-target dimensions, the floating panel, the marker projection
    /// ([`compute_panel_vp`](crate::hud::compute_panel_vp)), and the Scaleform viewport all derive
    /// from it, so they cannot drift out of sync.
    pub hud_aspect: f32,
    /// Aspect ratio (width / height) for full-screen UI -- movies, loading screens, and menus
    /// ([`HudMode::Movie`](crate::hud::HudMode)); `16:9` by default. See [`hud_aspect`](HudConfig::hud_aspect).
    pub movie_aspect: f32,
    /// HUD render-target scale, applied to the geometric mean of the **per-eye** render resolution.
    /// The texture's longer axis is `render_scale * sqrt(eye_width * eye_height)` pixels; the shorter
    /// axis follows from the effective aspect. Lower trades sharpness for fill rate. Ignored when
    /// [`render_resolution`](HudConfig::render_resolution) pins the size outright.
    ///
    /// The geometric mean, rather than the larger axis, keeps the HUD's pixel budget stable as the
    /// render target's shape changes: a per-eye target is much taller than it is wide, and keying off
    /// the longer axis would inflate the HUD for a shape the HUD does not share.
    pub render_scale: f32,
    /// Pin the HUD render target's longer axis to an explicit pixel count, ignoring
    /// [`render_scale`](HudConfig::render_scale) and the render resolution entirely. `None` derives
    /// the size as described on [`render_scale`](HudConfig::render_scale).
    ///
    /// The HUD is a flat panel viewed at a fixed apparent size, so what it actually needs is enough
    /// texels to look crisp at that size -- a property of the headset and the panel's angular size,
    /// not of how many pixels the scene happens to be rendered at. Pinning it makes that budget
    /// explicit and immune to render-resolution changes.
    #[serde(default)]
    pub render_resolution: Option<u32>,
    /// Distance from the eye to the panel, in meters. The panel resizes with distance to keep a
    /// constant apparent (angular) size, so this can be changed freely without the HUD growing or
    /// shrinking. Comfort band: 1.5-3m.
    pub distance: f32,
    /// Apparent-size multiplier for the panel; `1.0` is the comfortable baseline (4 m wide at 3 m).
    /// The physical size is derived from this, [`distance`](HudConfig::distance), and the effective
    /// aspect (see [`crate::hud::panel_height`]), so changing the distance or aspect keeps the panel
    /// looking the same size and fitting the same horizontal content.
    pub panel_scale: f32,
    /// Lazy-follow damping parameters for the floating panel.
    pub follow: FollowConfig,
    /// Distance from the eye to the world-marker layer while splitting, in meters. Markers keep a
    /// constant apparent size (like the panel), so this only changes their stereo depth. The
    /// per-marker depth warp supersedes this as markers' effective depth when enabled.
    pub marker_distance: f32,
    /// Distance from the eye to the screen-center (reticle) layer while splitting, in meters.
    /// Constant apparent size; the fallback (and easing target) while
    /// [`center_depth_from_aim`](HudConfig::center_depth_from_aim) has no recent aim point.
    pub center_distance: f32,
    /// Drive the center layer's depth from the grapple reticle's aim point (smoothed), so the
    /// reticle group sits at the vergence of the surface it targets.
    pub center_depth_from_aim: bool,
    /// Warp the panel per element: each on-screen world marker's neighborhood is displaced to
    /// the marker's real world depth (recorded from the game's own world-to-screen calls), and
    /// the screen-center reticle region to the aim depth, giving depth-correct stereo disparity
    /// without re-rendering the HUD. Applies to the single panel; while
    /// [`split`](HudConfig::split) is on it applies to the marker layer instead.
    pub marker_warp: bool,
    /// The radius of the center (reticle) region displaced to the aim depth, in texture-uv units.
    pub center_bubble_radius: f32,
    /// The warp falloff radius around each marker, in texture-uv units.
    pub marker_radius: f32,
    /// Marker depths are clamped to this, in meters -- beyond it disparity is indistinguishable
    /// from infinity.
    pub marker_max_depth: f32,
    /// PARKED, off by default: split the HUD into three depth layers -- static HUD, world
    /// markers, reticles -- each in its own texture composited at its own depth, at full rate
    /// (the movie's render tree partitioned across extra render roots). Gameplay works, but the
    /// first pause permanently stops the UI update pump; see the post-mortem in
    /// `docs/issues/08-14-hud-overlays-and-depth.md` and `payload/src/hud/split/`.
    pub split: bool,
    /// Keep the full-screen Scaleform overlays -- drowning tint, damage flashes, directional
    /// damage indicators -- hidden (issue #8): they were authored to cover a flat screen and
    /// cover the whole panel in VR instead. Enforced per frame on the game thread through the
    /// discovered clip handles, ahead of each capture.
    pub suppress_overlays: bool,
    /// World-lock the panel while an in-game modal menu (pause / full-screen map) is open: snapshot
    /// the panel pose the moment the menu opens and hold it there, so the player can look around a
    /// stationary panel instead of it chasing the head. Reverts to head-follow on close. Only in-game
    /// menus (`E_GAME_RUN` + a static UI background) freeze; the frontend and loading screens are
    /// excluded.
    #[serde(default)]
    pub world_lock_menus: bool,
    /// How the aim reticle is projected onto the panel, for tuning crosshair-vs-shot alignment (issue
    /// #6). The reticle is projected from the world aim point; the panel-subtense projection lands it
    /// where the head->aim ray crosses the panel, while the game-projection option follows the game's
    /// own screen-space mapping. Markers are unaffected (always panel-subtense).
    #[serde(default)]
    pub reticle_align: ReticleAlign,
    /// Dynamic panel distance from the scene depth distribution.
    pub depth_shift: DepthShiftConfig,
    /// The virtual mouse cursor for panel UI interaction (issue #9). See
    /// [`crate::hud::cursor`].
    #[serde(default)]
    pub cursor: CursorConfig,
    /// The clip-path prefix from the root movie's timeline to the HUD movie's clips, ending in a
    /// dot when non-empty (e.g. `"hud."`). The HUD movie is attached by `root.gfx`'s ActionScript
    /// under a runtime name the display-tree dump reveals; until confirmed in-game, the authored
    /// clip names are tried bare. Discovered at runtime, so it is not saved.
    #[serde(skip)]
    pub split_path_prefix: SplitPathPrefix,
    /// The interactive egui debug panel floating in 3D space in VR (issue #24). Off by default;
    /// only takes effect while an OpenXR session is running.
    #[serde(default)]
    pub egui_panel: EguiPanelConfig,
    /// Whether the flat egui debug overlay draws onto the desktop window: both the direct
    /// back-buffer path (`EguiState::render`, no VR session or the panel path inactive) and the
    /// deferred mirror-overlay texture (`crate::hud::mirror_overlay`, a VR session with the mirror
    /// on and [`egui_panel`](HudConfig::egui_panel) off). Off by default; F4 toggles it. Off hides the
    /// overlay from the desktop image only -- the VR floating panel, the headset image, and the in-world
    /// overlays are unaffected.
    #[serde(default)]
    pub flat_overlay_visible: bool,
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
            hud_aspect: 1.3,
            movie_aspect: 16.0 / 9.0,
            render_scale: 1.5,
            render_resolution: None,
            distance: 6.0,
            panel_scale: 0.8,
            follow: FollowConfig::new(),
            marker_distance: 3.0,
            center_distance: 3.0,
            center_depth_from_aim: true,
            marker_warp: false,
            center_bubble_radius: 0.12,
            marker_radius: 0.08,
            marker_max_depth: 150.0,
            split: false,
            depth_shift: DepthShiftConfig::new(),
            cursor: CursorConfig::new(),
            suppress_overlays: true,
            world_lock_menus: true,
            reticle_align: ReticleAlign::PanelSubtense,
            split_path_prefix: SplitPathPrefix::new(),
            egui_panel: EguiPanelConfig::new(),
            flat_overlay_visible: false,
            world_markers: WorldMarkersConfig::new(),
            atlas: AtlasConfig::new(),
        }
    }
}

/// How the aim reticle is projected onto the floating panel (issue #6). An A/B knob for the
/// crosshair-vs-shot alignment; only the reticle projection is affected, not the world markers.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReticleAlign {
    /// The panel's own angular subtense from the head pose (symmetric, off-center shear dropped) --
    /// the reticle lands where the head-to-aim ray crosses the panel. The shipped default.
    #[default]
    PanelSubtense,
    /// The game camera's own projection (off-center shear and game FOV kept) applied at the panel
    /// pose -- the reticle follows the game's native screen-space aim mapping instead.
    GameProjection,
}

/// The world-marker prototype ([`crate::hud::world_markers`]): each on-screen point-of-interest
/// marker is cropped out of the HUD texture and drawn as a billboard at its world point, keeping
/// the angular size it has on the panel, and its rectangle is cut out of the panel.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldMarkersConfig {
    /// Master toggle.
    pub enabled: bool,
    /// Decide whether a marker is on screen with the headset's field of view rather than the
    /// panel's narrower subtense, so a marker the player can see in the world is not demoted to an
    /// edge arrow. Markers outside the headset's view stay edge arrows on the panel.
    pub classify_with_headset_fov: bool,
    /// The half-angle of the classification field of view, in degrees, used when no headset frame
    /// supplies one (the flat desktop).
    pub fallback_half_fov_deg: f32,
    /// How far inside the headset's field of view a marker must be to count as on screen, in
    /// degrees, so a marker at the very edge of the lenses keeps its arrow.
    pub fov_inset_deg: f32,
    /// Cut each world-placed marker's rectangle out of the panel.
    pub hide_from_panel: bool,
    /// The cropped rectangle around a marker clip's origin, in stage pixels.
    pub crop: CropExtents,
    /// The extra width to the left of the labelled marker's crop, in stage pixels: the label sits
    /// to the left of the icon.
    pub label_extra_left: f32,
    /// The farthest a billboard is placed, in metres. A farther marker is drawn at this distance
    /// along its direction, where its stereo disparity is already indistinguishable from infinity.
    pub max_distance_m: f32,
    /// Place each marker on the closest visible surface along the line of sight to its point (the
    /// scene depth at its pixel, or a raycast where no depth sample is available), so it never
    /// reads as farther than a wall it shows through. Off places markers at their true points.
    pub surface_placement: bool,
    /// Fall back to a physics raycast for markers without a recent depth sample.
    pub ray_fallback: bool,
    /// How far a surface-placed marker is pulled toward the eye from the surface, in metres, so it
    /// is not coplanar with it.
    pub surface_pull_m: f32,
    /// Surfaces nearer the eye than this are ignored for the placement, in metres: the player's
    /// own gun and hands, and foliage brushing the head, would otherwise pull every marker behind
    /// them onto them.
    pub surface_min_m: f32,
    /// The halflife of the placement distance's smoothing, in seconds, so a flickering occluder
    /// does not make the marker pop.
    pub depth_halflife_s: f32,
    /// EXPERIMENT (the atlas viewport probe): compute the gameplay HUD's safe area against the
    /// stage's own aspect instead of the panel texture's, so the HUD lays out inside the stage and
    /// the bands the show-all scaling exposes above and below it stay empty. Applies only in
    /// gameplay; menus keep the full layout.
    pub probe_stage_safe_area: bool,
}

impl WorldMarkersConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            classify_with_headset_fov: true,
            fallback_half_fov_deg: 50.0,
            fov_inset_deg: 2.0,
            hide_from_panel: true,
            crop: CropExtents::new(),
            label_extra_left: 320.0,
            max_distance_m: 300.0,
            surface_placement: true,
            ray_fallback: true,
            surface_pull_m: 0.3,
            surface_min_m: 1.2,
            depth_halflife_s: 0.15,
            probe_stage_safe_area: false,
        }
    }
}

impl Default for WorldMarkersConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The HUD atlas ([`crate::hud::atlas`]): while it is on, the gameplay HUD lays out inside the
/// stage (as [`WorldMarkersConfig::probe_stage_safe_area`] does), and at each capture the parts below
/// are moved into cells in the bands above and below the stage, then moved back.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AtlasConfig {
    /// Master toggle.
    pub enabled: bool,
    /// The empty margin around every cell, in stage pixels, wider than the glows and shadows the
    /// moved parts draw past their bounds.
    pub gutter_px: f32,
    /// Move the world-placed markers into cells instead of cropping them from the panel (with
    /// [`WorldMarkersConfig::enabled`]). Markers that find no cell are cropped as before.
    pub markers: bool,
    /// The ammunition readout above the gun hand.
    pub ammo: AmmoConfig,
    /// The health and heat readout on the off-hand's wrist.
    pub wrist: WristStatusConfig,
    /// The weapon selection cross at the hand steering the weapon wheel.
    pub cross: CrossConfig,
    /// The tether readout above the off-hand's forearm.
    pub tether: TetherConfig,
}

impl AtlasConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            gutter_px: 6.0,
            markers: true,
            ammo: AmmoConfig::new(),
            wrist: WristStatusConfig::new(),
            cross: CrossConfig::new(),
            tether: TetherConfig::new(),
        }
    }
}

impl Default for AtlasConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The weapon readout (`MCI_weapons_parent`: the weapon's name and icon, the magazine bar, and the
/// reserve) drawn above the hand that holds the gun, under the controller scheme, upright and
/// facing the eye. The game shows it while a gun is out and fades it when holstered; the quad
/// carries that fade.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AmmoConfig {
    pub enabled: bool,
    /// The readout's extents around its clip's origin, in the clip's own pixels.
    pub extents: CropExtents,
    /// The largest scale the readout is captured at; it is scaled down further to fit a band.
    pub max_scale: f32,
    /// How far above the gun hand's grip the quad's lower edge sits, metres, straight up in the
    /// world whatever the hand's tilt, so the quad clears the hand and the gun in it.
    pub clearance_m: f32,
    /// How far the quad is pulled from above the hand toward the head, metres.
    pub pull_m: f32,
    /// The quad's width, metres; its height follows the cell's aspect.
    pub width_m: f32,
    /// The halflife of the quad's follow of the hand, seconds.
    pub follow_halflife_s: f32,
    /// How long the quad stays on the last gun hand after no hand holds a gun, seconds, so the
    /// game's fade-out plays where the gun was.
    pub linger_s: f32,
}

impl AmmoConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            extents: CropExtents {
                left: 25.0,
                right: 285.0,
                up: 90.0,
                down: 175.0,
            },
            max_scale: 1.0,
            clearance_m: 0.10,
            pull_m: 0.02,
            width_m: 0.10,
            follow_halflife_s: 0.05,
            linger_s: 1.5,
        }
    }
}

impl Default for AmmoConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The game's weapon selection cross (`MCI_weapon_selection_wheel`) drawn where the hand steering
/// the weapon wheel was when the wheel opened, under the controller scheme, while the wheel shows
/// it and for its hide animation after. Without the atlas, the wheel places the cross on the panel
/// instead.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CrossConfig {
    pub enabled: bool,
    /// Room added around the cross's own extent (which the game reports), in its own pixels, for
    /// its glows and its arms' highlight animations.
    pub margin_px: f32,
    /// The largest scale the cross is captured at; it is scaled down further to fit a band.
    pub max_scale: f32,
    /// The quad's width, metres; its height follows the cell's aspect.
    pub width_m: f32,
    /// How far the quad is pulled from the hand toward the head, metres, so the hand does not
    /// cover it.
    pub pull_m: f32,
}

impl CrossConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            margin_px: 12.0,
            max_scale: 1.0,
            width_m: 0.18,
            pull_m: 0.08,
        }
    }
}

impl Default for CrossConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The game's tether readout (`MCI_dual_tether`: the count of tethers in use out of the most the
/// grapple holds, and the retract state) drawn above the off-hand's forearm, where the grapple
/// fires from, under the controller scheme, upright and facing the eye. The game shows it while
/// tethers are attached and fades it out after; the quad carries that fade.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TetherConfig {
    pub enabled: bool,
    /// The readout's extents around its clip's origin, in the clip's own pixels.
    pub extents: CropExtents,
    /// The largest scale the readout is captured at; it is scaled down further to fit a band.
    pub max_scale: f32,
    /// How far back along the forearm from the wrist the quad sits, metres.
    pub back_m: f32,
    /// How far above the forearm the quad's lower edge sits, metres, straight up in the world.
    pub clearance_m: f32,
    /// The quad's width, metres; its height follows the cell's aspect.
    pub width_m: f32,
    /// The halflife of the quad's follow of the forearm, seconds.
    pub follow_halflife_s: f32,
}

impl TetherConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            extents: CropExtents {
                left: 60.0,
                right: 60.0,
                up: 25.0,
                down: 30.0,
            },
            max_scale: 1.0,
            back_m: 0.12,
            clearance_m: 0.06,
            width_m: 0.08,
            follow_halflife_s: 0.05,
        }
    }
}

impl Default for TetherConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// When the wrist readout shows.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WristVisibility {
    /// Always, while the wrist is tracked.
    Always,
    /// While the player glances at the inside of the wrist.
    Glance,
    /// On a glance, and also on its own while health is low, for a while after damage, and while
    /// the player has heat.
    Auto,
}

/// The health bar (drawn by the mod from the player character's health) and the heat readout
/// (`MCI_heat_container`, moved into a cell) on the inside of the off-hand's wrist, under the
/// controller scheme. The controls summary sits on the back of the same wrist.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WristStatusConfig {
    pub enabled: bool,
    pub visibility: WristVisibility,
    /// Show the health bar.
    pub health: bool,
    /// Show the heat readout.
    pub heat: bool,
    /// The heat readout's extents around its clip's origin, in the clip's own pixels.
    pub heat_extents: CropExtents,
    /// The largest scale the heat readout is captured at; it is scaled down further to fit a band.
    pub heat_max_scale: f32,
    /// The panel's centre in the grip frame, metres: +X out of the palm, +Z back along the forearm
    /// (mirrored on X between the hands, so +X is the palm side of either). The default sits
    /// toward the elbow from the C4 detonator button on the same side of the wrist.
    pub offset_m: Vec3,
    /// The panel's width, metres.
    pub width_m: f32,
    /// The health bar's width over its height.
    pub bar_aspect: f32,
    /// The glance that shows it.
    pub glance: GlanceConfig,
    /// The fade in and out, seconds.
    pub fade_s: f32,
    /// How long it shows after damage under [`WristVisibility::Auto`], seconds.
    pub after_damage_s: f32,
    /// The health fraction at and below which it shows under [`WristVisibility::Auto`].
    pub low_health: f32,
}

impl WristStatusConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            visibility: WristVisibility::Auto,
            health: true,
            heat: true,
            heat_extents: CropExtents {
                left: 140.0,
                right: 170.0,
                up: 110.0,
                down: 30.0,
            },
            heat_max_scale: 1.0,
            offset_m: Vec3::new(0.04, 0.0, 0.16),
            width_m: 0.08,
            bar_aspect: 10.0,
            glance: GlanceConfig::new(),
            fade_s: 0.15,
            after_damage_s: 4.0,
            low_health: 0.35,
        }
    }
}

impl Default for WristStatusConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A rectangle around a point, as its extents on each side, in stage pixels.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CropExtents {
    pub left: f32,
    pub right: f32,
    pub up: f32,
    pub down: f32,
}

impl CropExtents {
    pub const fn new() -> Self {
        Self {
            left: 40.0,
            right: 90.0,
            up: 44.0,
            down: 44.0,
        }
    }
}

impl Default for CropExtents {
    fn default() -> Self {
        Self::new()
    }
}

/// The interactive egui debug panel rendered as a 2D surface floating in 3D space in VR (issue #24).
///
/// The panel is an independent floating surface: it has its own offscreen render target and its own
/// placement in the player's tracked space, so it never perturbs the gameplay HUD. It only takes
/// effect while an OpenXR session is running; on the flat desktop the egui debug overlay stays on the
/// back buffer as before.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EguiPanelConfig {
    /// Master toggle. When on (and a session is running), the egui debug UI renders into the panel
    /// texture and floats in the player's tracked space instead of on the flat back buffer. Off by
    /// default so the working baseline is never disturbed.
    pub enabled: bool,
    /// How far in front of the head the panel is placed when it is summoned, in meters. The panel's
    /// physical size is the one that gives it the apparent size [`scale`](EguiPanelConfig::scale) at
    /// this distance.
    pub distance: f32,
    /// Apparent-size multiplier for the panel at the summoning distance; `1.0` is the comfortable
    /// baseline (see [`crate::hud::panel_height`]).
    pub scale: f32,
    /// Aspect ratio (width / height) of the panel quad. Kept in step with
    /// [`resolution`](EguiPanelConfig::resolution) so the egui content is not stretched.
    pub aspect: f32,
    /// The panel texture's pixel resolution. egui lays out at this size, so it also sets the effective
    /// DPI of the panel content.
    pub resolution: (u32, u32),
    /// Whether to also composite the panel onto the desktop mirror while a VR session runs. Off by
    /// default: the panel is already visible in the headset, so the desktop mirror shows the
    /// unobstructed scene. Turn it on for a legible desktop copy of the panel.
    pub show_on_mirror: bool,
    /// Whether the controllers point at the panel: either hand's aim ray moves the pointer, its
    /// trigger clicks, its upper face button right-clicks, and its thumbstick scrolls. The desktop
    /// mouse keeps working alongside; whichever was used last owns the pointer. On by default.
    pub controller_pointer: bool,
    /// How far a controller's held trigger press may wander from where it began, as a fraction of
    /// the panel's height, and still click: until the hit leaves this radius the pointer stays at
    /// the press, so a hand's tremor never turns a press into a drag. Past it the press drags from
    /// the live hit.
    pub click_slop: f32,
    /// How long a press may be held and still click, seconds (egui's
    /// [`max_click_duration`](egui::InputOptions::max_click_duration)), while the panel is shown.
    /// egui's own default is under a second, which a deliberate press in the headset outlasts.
    pub click_duration_s: f32,
}

impl EguiPanelConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            distance: 2.5,
            scale: 0.85,
            aspect: 4.0 / 3.0,
            resolution: (1280, 960),
            show_on_mirror: false,
            controller_pointer: true,
            click_slop: 0.03,
            click_duration_s: 2.0,
        }
    }
}

impl Default for EguiPanelConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Dynamic panel distance from the scene depth distribution (issue #14): a compute pass
/// histograms the whole main depth buffer each frame, and the panel eases toward
/// [`near_distance`](DepthShiftConfig::near_distance) while enough of the frame sits nearer than
/// [`near_threshold`](DepthShiftConfig::near_threshold) (a vehicle interior, a corridor, a
/// wall), back to the base [`distance`](HudConfig::distance) otherwise, and always far during
/// full-screen UI. See `payload/src/hud/depth.rs`.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DepthShiftConfig {
    /// Master toggle.
    pub enabled: bool,
    /// Drive the near shift from the scene depth histogram instead of the vehicle state. The
    /// vehicle flag (the default) is deterministic -- near while riding, base otherwise -- and
    /// immune to the histogram's false positives (a first-person weapon filling the view); the
    /// histogram generalizes to corridors and interiors the flag cannot see.
    pub use_depth_histogram: bool,
    /// Depths nearer than this count as near-field, in meters.
    pub near_threshold: f32,
    /// The fraction of the frame that must be near-field to engage the near shift.
    pub near_occupancy: f32,
    /// The occupancy slack below the engage level before the shift releases (hysteresis, so the
    /// panel does not flap at the boundary).
    pub hysteresis: f32,
    /// The panel distance while the near shift is engaged, in meters.
    pub near_distance: f32,
    /// The easing halflife between distances, in seconds.
    pub halflife: f32,
    /// EXPERIMENTAL: follow the scene continuously instead of the threshold policy -- the panel
    /// sits [`margin`](DepthShiftConfig::margin) inside the configured percentile of the depth
    /// distribution, clamped to [`near_distance`](DepthShiftConfig::near_distance) and the base
    /// distance.
    pub continuous: bool,
    /// The depth percentile the continuous policy follows (0-1).
    pub percentile: f32,
    /// How far inside the percentile depth the continuous policy sits, in meters.
    pub margin: f32,
    /// Sample every Nth pixel of the depth buffer on both axes.
    pub sample_stride: u32,
    /// Weight histogram samples by the HUD texture's alpha where the sample's camera ray meets
    /// the panel, so the statistics describe the depth behind visible HUD content rather than
    /// the whole frame.
    pub mask_by_hud: bool,
    /// Ignore depth samples nearer than this, in meters (a floor for first-person viewmodel
    /// geometry; 0 disables).
    pub min_depth: f32,
}

impl DepthShiftConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            use_depth_histogram: false,
            near_threshold: 2.0,
            near_occupancy: 0.2,
            hysteresis: 0.05,
            near_distance: 1.1,
            halflife: 0.35,
            continuous: false,
            percentile: 0.10,
            margin: 0.3,
            sample_stride: 4,
            mask_by_hud: true,
            min_depth: 0.0,
        }
    }
}

impl Default for DepthShiftConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The virtual mouse cursor rendered on the floating panel (issue #9): a circle dot with a stroke,
/// lifted slightly off the panel toward the camera. Position injection and visibility follow the
/// game's own cursor policy; see [`crate::hud::cursor`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CursorConfig {
    /// Master toggle: replace the game's mouse-to-UI coordinate mapping (broken by the redirect)
    /// and render the panel cursor.
    pub enabled: bool,
    /// The cursor's diameter as a fraction of the panel distance, so it keeps a constant apparent
    /// (angular) size like the panel itself.
    pub size: f32,
    /// How far the cursor floats off the panel surface toward the camera, in meters. The offset
    /// gives it depth separation from the UI beneath it.
    pub lift: f32,
}

impl CursorConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            size: 0.014,
            lift: 0.03,
        }
    }
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A short fixed-capacity string for [`HudConfig::split_path_prefix`], so `HudConfig` stays `Copy`
/// (the config is snapshotted by value throughout the payload).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SplitPathPrefix {
    bytes: [u8; Self::CAPACITY],
    len: u8,
}

impl Default for SplitPathPrefix {
    fn default() -> Self {
        Self::new()
    }
}

impl SplitPathPrefix {
    /// Enough for a couple of nesting levels of clip names.
    pub const CAPACITY: usize = 64;

    pub const fn new() -> Self {
        Self {
            bytes: [0; Self::CAPACITY],
            len: 0,
        }
    }

    pub fn as_str(&self) -> &str {
        // The only writers validated UTF-8 on the way in.
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }

    /// Replace the prefix. Fails when the string exceeds the capacity.
    pub fn set(&mut self, value: &str) -> Result<(), PrefixTooLongError> {
        let bytes = value.as_bytes();
        if bytes.len() > Self::CAPACITY {
            return Err(PrefixTooLongError { len: bytes.len() });
        }
        self.bytes = [0; Self::CAPACITY];
        self.bytes[..bytes.len()].copy_from_slice(bytes);
        self.len = bytes.len() as u8;
        Ok(())
    }
}

impl TryFrom<String> for SplitPathPrefix {
    type Error = PrefixTooLongError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let mut prefix = Self::new();
        prefix.set(&value)?;
        Ok(prefix)
    }
}

impl From<SplitPathPrefix> for String {
    fn from(value: SplitPathPrefix) -> Self {
        value.as_str().to_string()
    }
}

/// The clip-path prefix exceeds [`SplitPathPrefix::CAPACITY`].
#[derive(Debug)]
pub struct PrefixTooLongError {
    len: usize,
}

impl std::fmt::Display for PrefixTooLongError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "hud config: the split path prefix is {} bytes; the capacity is {}",
            self.len,
            SplitPathPrefix::CAPACITY
        )
    }
}

impl std::error::Error for PrefixTooLongError {}

/// Lazy-follow damping parameters for the floating HUD panel. See `docs/mod/hud.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FollowConfig {
    /// Rotation follow halflife in seconds. Lower = snappier follow.
    pub rotation_halflife: f32,
    /// Position de-jitter halflife in seconds.
    pub position_halflife: f32,
}
impl FollowConfig {
    pub const fn new() -> Self {
        Self {
            rotation_halflife: 0.2,
            position_halflife: 0.1,
        }
    }
}
