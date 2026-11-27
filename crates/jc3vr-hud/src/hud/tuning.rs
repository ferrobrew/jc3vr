//! The HUD's tuning, read from the shipped `tuning/hud.toml` ([`HUD`]): the floating panel's shape
//! and follow, the cursor, the aim depth, the debug panel's placement and pointer, the controls
//! map's pointer and zoom, the body-anchored readouts, the world markers, the grip markers' labels,
//! the lost weapons' markers, the grab hints, the game's reticles, the controls summary's timing,
//! and the restart notice (`docs/mod/tuning.md`).

use std::time::Duration;

use glam::Vec3;
use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range, vec3},
    published::Window,
    tuning::Tuning,
};
use jc3vr_gameplay::controllers::summary::config::{GlanceConfig, WristPlacement};
use serde::Deserialize;

use crate::hud::lost_weapons::MAX_TEXTURES;

jc3vr_core::tuning_file! {
    /// The HUD's tuning file.
    pub static HUD: HudTuning = "hud.toml";
}

/// The HUD's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HudTuning {
    pub panel: PanelTuning,
    pub cursor: CursorTuning,
    pub aim: AimTuning,
    pub egui_panel: EguiPanelTuning,
    pub map: MapTuning,
    pub anchored: AnchoredTuning,
    pub world_markers: WorldMarkersTuning,
    pub(crate) mark_labels: MarkLabelsTuning,
    pub lost_weapons: LostWeaponsTuning,
    pub grab_hint: GrabHintTuning,
    pub(crate) game_reticle: GameReticleTuning,
    pub summary: SummaryTuning,
    pub(crate) restart_notice: RestartNoticeTuning,
}

impl Tuning for HudTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.panel.validate()?;
        positive("cursor.size", self.cursor.size, 1.0)?;
        nonnegative("cursor.lift_m", self.cursor.lift_m, 1.0)?;
        self.aim.validate()?;
        self.egui_panel.validate()?;
        self.map.validate()?;
        self.anchored.validate()?;
        self.world_markers.validate()?;
        self.mark_labels.validate()?;
        self.lost_weapons.validate()?;
        self.grab_hint.validate()?;
        millis(
            "game_reticle.aim_max_age_ms",
            self.game_reticle.aim_max_age_ms,
        )?;
        self.summary.validate()?;
        self.restart_notice.validate()
    }
}

/// The floating panel (see [`crate::hud`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelTuning {
    /// The gameplay HUD's aspect (width / height); `1.0` is square. The effective aspect for the
    /// frame (this, or the movies' 16:9 for full-screen UI) is the single source of truth for the
    /// HUD's shape: the render-target dimensions, the floating panel, the marker projection, and the
    /// Scaleform viewport all derive from it, so they cannot drift out of sync.
    pub(crate) hud_aspect: f32,
    /// The HUD render target's scale, applied to the geometric mean of the per-eye render
    /// resolution: the texture's longer axis is `render_scale * sqrt(eye_width * eye_height)`
    /// pixels, and the shorter axis follows from the effective aspect. The geometric mean, rather
    /// than the larger axis, keeps the HUD's pixel budget stable as the render target's shape
    /// changes: a per-eye target is much taller than it is wide, and keying off the longer axis
    /// would inflate the HUD for a shape the HUD does not share.
    pub(crate) render_scale: f32,
    /// The panel's rotation follow halflife, seconds. Lower is a snappier follow.
    pub(crate) follow_rotation_halflife_s: f32,
    /// How the panel's distance follows the vehicle state ([`crate::hud::depth`]).
    pub(crate) depth_shift: DepthShiftTuning,
    /// How long a sampled stage safe area stays current without a newer one, milliseconds.
    pub(crate) stage_safe_area_stale_ms: u64,
}

impl PanelTuning {
    /// [`PanelTuning::stage_safe_area_stale_ms`] as a duration.
    pub(crate) fn stage_safe_area_stale(&self) -> Duration {
        Duration::from_millis(self.stage_safe_area_stale_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        positive("panel.hud_aspect", self.hud_aspect, 10.0)?;
        positive("panel.render_scale", self.render_scale, 8.0)?;
        positive(
            "panel.follow_rotation_halflife_s",
            self.follow_rotation_halflife_s,
            10.0,
        )?;
        positive(
            "panel.depth_shift.near_distance_m",
            self.depth_shift.near_distance_m,
            100.0,
        )?;
        positive(
            "panel.depth_shift.halflife_s",
            self.depth_shift.halflife_s,
            10.0,
        )?;
        millis(
            "panel.stage_safe_area_stale_ms",
            self.stage_safe_area_stale_ms,
        )
    }
}

/// How the panel distance follows the vehicle state (see [`crate::hud::depth`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DepthShiftTuning {
    /// The panel distance while the near shift is engaged, metres.
    pub(crate) near_distance_m: f32,
    /// The easing halflife between distances, seconds.
    pub(crate) halflife_s: f32,
}

/// The virtual mouse cursor drawn on the panel (see [`crate::hud::cursor`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CursorTuning {
    /// The cursor's diameter as a fraction of the panel distance, so it keeps a constant apparent
    /// (angular) size like the panel itself.
    pub size: f32,
    /// How far the cursor floats off the panel surface toward the camera, metres. The offset gives
    /// it depth separation from the UI beneath it.
    pub(crate) lift_m: f32,
}

/// The aim depth (see [`crate::hud::aim`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AimTuning {
    /// The nearest recorded aim depth, metres.
    pub(crate) min_depth_m: f32,
    /// The farthest recorded aim depth, metres: beyond it, disparity is indistinguishable from
    /// infinity.
    pub(crate) max_depth_m: f32,
    /// The distance the aim depth eases back to while nothing has been aimed at recently, metres.
    pub(crate) rest_distance_m: f32,
    /// How long a recorded aim point stays authoritative, seconds. Beyond this, the smoothed depth
    /// eases back to the caller's base distance and the aim point reads as none (the reticle is not
    /// targeting anything).
    pub(crate) stale_after_s: f32,
    /// The smoothing halflife, seconds: long enough to swallow surface noise as the aim sweeps
    /// across geometry, short enough to track a real target change quickly.
    pub(crate) halflife_s: f32,
}

impl AimTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("aim.max_depth_m", self.max_depth_m, 10_000.0)?;
        range("aim.min_depth_m", self.min_depth_m, 0.0, self.max_depth_m)?;
        range(
            "aim.rest_distance_m",
            self.rest_distance_m,
            self.min_depth_m,
            self.max_depth_m,
        )?;
        nonnegative("aim.stale_after_s", self.stale_after_s, 10.0)?;
        positive("aim.halflife_s", self.halflife_s, 10.0)
    }
}

/// The interactive debug panel floating in the world (see [`crate::hud::egui_panel`] and
/// [`crate::hud::pointer`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EguiPanelTuning {
    /// How far in front of the head the panel is placed when it is summoned, metres. The panel's
    /// physical size is the one that gives it the apparent size
    /// [`scale`](EguiPanelTuning::scale) at this distance.
    pub(crate) summon_distance_m: f32,
    /// The panel's apparent-size multiplier at the summoning distance; `1.0` is the comfortable
    /// baseline (see `crate::hud::panel_height`).
    pub scale: f32,
    /// The nearest a summoned or grabbed panel is placed, metres.
    pub min_distance_m: f32,
    /// The farthest the thumbstick pushes a grabbed panel along the hand's aim ray, metres.
    pub(crate) max_push_depth_m: f32,
    /// How fast a full deflection pushes or pulls a grabbed panel, as a fraction of its depth along
    /// the ray per second, so the push is as quick to read at arm's length as across a room.
    pub(crate) push_rate_per_s: f32,
    /// How long a press may be held and still click, seconds (egui's
    /// [`max_click_duration`](egui::InputOptions::max_click_duration)), while the panel is shown.
    /// egui's own default is under a second, which a deliberate press in the headset outlasts.
    pub(crate) click_duration_s: f64,
    /// How far, in panel UV, the controller's ray must move from where it rested while the desktop
    /// owned the pointer before the controller takes the pointer over.
    pub(crate) controller_move_uv: f32,
}

impl EguiPanelTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive(
            "egui_panel.summon_distance_m",
            self.summon_distance_m,
            100.0,
        )?;
        positive("egui_panel.scale", self.scale, 100.0)?;
        positive("egui_panel.max_push_depth_m", self.max_push_depth_m, 1000.0)?;
        range(
            "egui_panel.min_distance_m",
            self.min_distance_m,
            f32::EPSILON,
            self.max_push_depth_m,
        )?;
        positive("egui_panel.push_rate_per_s", self.push_rate_per_s, 100.0)?;
        positive(
            "egui_panel.click_duration_s",
            self.click_duration_s as f32,
            60.0,
        )?;
        nonnegative(
            "egui_panel.controller_move_uv",
            self.controller_move_uv,
            1.0,
        )
    }
}

/// The controls map's pointer and stick zoom (see [`crate::controllers::map`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapTuning {
    /// How far, in panel UV, the ray may wander from a primary press before the map sees the
    /// cursor move. The map turns a held press into a drag once its cursor leaves the press point by
    /// a few stage pixels, and a hand-held ray moves further than that from tremor and from the
    /// trigger pull itself, so without the slop nearly every trigger pull would start a drag, and
    /// its release would end the drag instead of selecting.
    pub click_slop_uv: f32,
    /// How far the stick must be pushed to start zooming.
    pub(crate) zoom_engage: f32,
    /// How far the stick must return before a new push counts; the gap to
    /// [`zoom_engage`](MapTuning::zoom_engage) is the hysteresis that keeps a stick resting near
    /// the threshold from chattering.
    pub(crate) zoom_release: f32,
    /// How long a held push waits before it repeats, milliseconds.
    pub(crate) zoom_repeat_delay_ms: u64,
    /// The interval between repeated steps while the push is held, milliseconds.
    pub(crate) zoom_repeat_interval_ms: u64,
}

impl MapTuning {
    /// [`MapTuning::zoom_repeat_delay_ms`] as a duration.
    pub(crate) fn zoom_repeat_delay(&self) -> Duration {
        Duration::from_millis(self.zoom_repeat_delay_ms)
    }

    /// [`MapTuning::zoom_repeat_interval_ms`] as a duration.
    pub(crate) fn zoom_repeat_interval(&self) -> Duration {
        Duration::from_millis(self.zoom_repeat_interval_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("map.click_slop_uv", self.click_slop_uv, 1.0)?;
        positive("map.zoom_engage", self.zoom_engage, 1.0)?;
        range("map.zoom_release", self.zoom_release, 0.0, self.zoom_engage)?;
        millis("map.zoom_repeat_delay_ms", self.zoom_repeat_delay_ms)?;
        millis("map.zoom_repeat_interval_ms", self.zoom_repeat_interval_ms)?;
        if self.zoom_repeat_interval_ms == 0 {
            return Err(Invalid(
                "map.zoom_repeat_interval_ms must be above zero".to_owned(),
            ));
        }
        Ok(())
    }
}

/// The body-anchored readouts (see [`crate::hud::anchored`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchoredTuning {
    /// How far the forearm may jump in a frame before the tether readout snaps instead of easing,
    /// metres.
    pub snap_m: f32,
    /// How long an ammunition counter takes to fade out as the gun is raised to the eye, and back
    /// in, seconds.
    pub(crate) sight_fade_s: f32,
    /// How near the eye the gun hand's grip is when the gun is raised to sight along it, metres.
    pub(crate) sight_reach_m: f32,
    /// How far off the gaze the gun may point and still be sighted along, degrees.
    pub(crate) sight_cone_deg: f32,
    /// The range of the vertical component of the direction to the eye over which an upright
    /// quad's up blends from the world's up to the head's (`0.95` is about 18 degrees off vertical,
    /// `0.995` about 6).
    pub(crate) upright_blend: (f32, f32),
    /// The ammunition counter above each gun.
    pub ammo: AmmoTuning,
    /// The tether readout above the off-hand's forearm.
    pub tether: TetherTuning,
    /// The weapon selection cross.
    pub cross: CrossTuning,
    /// The readouts on the inside of the off-hand's wrist.
    pub wrist: WristTuning,
    /// The health bar's trail.
    pub(crate) health_trail: HealthTrailTuning,
}

impl AnchoredTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("anchored.snap_m", self.snap_m, 10.0)?;
        nonnegative("anchored.sight_fade_s", self.sight_fade_s, 10.0)?;
        nonnegative("anchored.sight_reach_m", self.sight_reach_m, 2.0)?;
        nonnegative("anchored.sight_cone_deg", self.sight_cone_deg, 90.0)?;
        let (level, vertical) = self.upright_blend;
        range("anchored.upright_blend[0]", level, 0.0, 1.0)?;
        range(
            "anchored.upright_blend[1]",
            vertical,
            level + f32::EPSILON,
            1.0,
        )?;

        let ammo = &self.ammo;
        nonnegative("anchored.ammo.gun_clearance_m", ammo.gun_clearance_m, 1.0)?;
        nonnegative("anchored.ammo.clearance_m", ammo.clearance_m, 1.0)?;
        positive("anchored.ammo.width_m", ammo.width_m, 1.0)?;

        let tether = &self.tether;
        range("anchored.tether.back_m", tether.back_m, -1.0, 1.0)?;
        range("anchored.tether.clearance_m", tether.clearance_m, -1.0, 1.0)?;
        positive("anchored.tether.width_m", tether.width_m, 1.0)?;
        positive(
            "anchored.tether.follow_halflife_s",
            tether.follow_halflife_s,
            10.0,
        )?;

        positive("anchored.cross.width_m", self.cross.width_m, 1.0)?;
        nonnegative("anchored.cross.pull_m", self.cross.pull_m, 1.0)?;

        let wrist = &self.wrist;
        vec3("anchored.wrist.offset_m", wrist.offset_m, 1.0)?;
        positive("anchored.wrist.width_m", wrist.width_m, 1.0)?;
        validate_glance("anchored.wrist.glance", &wrist.glance)?;
        nonnegative("anchored.wrist.gap_m", wrist.gap_m, 1.0)?;
        positive(
            "anchored.wrist.grenades_height_m",
            wrist.grenades_height_m,
            1.0,
        )?;
        nonnegative("anchored.wrist.fade_s", wrist.fade_s, 10.0)?;
        nonnegative("anchored.wrist.after_damage_s", wrist.after_damage_s, 60.0)?;
        range("anchored.wrist.low_health", wrist.low_health, 0.0, 1.0)?;

        nonnegative(
            "anchored.health_trail.hold_s",
            self.health_trail.hold_s,
            10.0,
        )?;
        positive(
            "anchored.health_trail.drain_per_s",
            self.health_trail.drain_per_s,
            100.0,
        )
    }
}

/// The ammunition counter above a gun in a hand.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmmoTuning {
    /// The gap above the model's local top, measured along the rendered gun's up axis, metres.
    pub(crate) gun_clearance_m: f32,
    /// The gap above the rendered gun's origin when its model box is unknown, metres.
    pub clearance_m: f32,
    /// The counter's width, metres; its height follows the texture's aspect.
    pub width_m: f32,
}

/// The tether readout above the off-hand's forearm.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TetherTuning {
    /// How far back along the forearm from the wrist the readout sits, metres.
    pub back_m: f32,
    /// How far above the forearm the readout's lower edge sits, metres, straight up in the world.
    pub clearance_m: f32,
    /// The readout's width, metres; its height follows the cell's aspect.
    pub width_m: f32,
    /// The halflife of the readout's follow of the forearm, seconds.
    pub(crate) follow_halflife_s: f32,
}

/// The weapon selection cross, where the hand steering the weapon wheel was when it opened.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossTuning {
    /// The cross's width, metres; its height follows the cell's aspect.
    pub width_m: f32,
    /// How far the cross is pulled from the hand toward the head, metres, so the hand does not
    /// cover it.
    pub pull_m: f32,
}

/// The health bar and the heat and grenade readouts on the inside of the off-hand's wrist.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WristTuning {
    /// The wrist panel's centre in the grip frame, metres: +X out of the palm, +Z back along the
    /// forearm (mirrored on X between the hands, so +X is the palm side of either).
    pub offset_m: Vec3,
    /// The wrist panel's width, metres.
    pub width_m: f32,
    /// The glance at the inside of the wrist that shows the wrist readout.
    pub glance: GlanceConfig,
    /// The gap between the stacked readouts, metres.
    pub gap_m: f32,
    /// The grenade readout's height, metres; its width follows the cell's aspect. The readout is a
    /// narrow column of icons, so it is sized by its height rather than the wrist's width.
    pub(crate) grenades_height_m: f32,
    /// The wrist readout's fade in and out, seconds.
    pub(crate) fade_s: f32,
    /// How long the wrist readout shows on its own after damage, seconds.
    pub(crate) after_damage_s: f32,
    /// The health fraction at and below which the wrist readout shows on its own, and the bar
    /// reads as low.
    pub(crate) low_health: f32,
}

/// The trail behind the health bar: where the bar stood before the last hit, held briefly and then
/// drained toward the health.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HealthTrailTuning {
    /// How long the trail holds after a hit before draining, seconds.
    pub hold_s: f32,
    /// How fast the trail drains, in health fractions per second.
    pub(crate) drain_per_s: f32,
}

/// The world markers drawn at their world points (see [`crate::hud::world_markers`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldMarkersTuning {
    /// The nearest a marker is drawn, metres.
    pub min_distance_m: f32,
    /// Surfaces nearer the eye than this are ignored for the placement, metres: the player's own
    /// gun and hands, and foliage brushing the head, would otherwise pull every marker behind them
    /// onto them.
    pub(crate) surface_min_m: f32,
    /// How far a surface-placed marker is pulled toward the eye from the surface, metres, so it is
    /// not coplanar with it.
    pub(crate) surface_pull_m: f32,
    /// The share of a marker's distance a surface must be nearer than for the marker to move onto
    /// it.
    pub(crate) surface_engage: f32,
    /// The share of a marker's distance a surface must reach for a marker on it to leave it.
    pub(crate) surface_release: f32,
    /// The halflife of the placement distance's smoothing, seconds, so a flickering occluder does
    /// not make the marker pop.
    pub(crate) depth_halflife_s: f32,
    /// The farthest a billboard is placed, metres. A farther marker is drawn at this distance along
    /// its direction, where its stereo disparity is already indistinguishable from infinity.
    pub max_distance_m: f32,
    /// How long a marker's placement state outlives its last sighting, seconds.
    pub(crate) forget_s: f32,
    /// How long a depth probe result stays usable, seconds, after which the marker falls back to
    /// the raycast.
    pub(crate) probe_ttl_s: f32,
    /// How long a raycast result is kept, seconds.
    pub(crate) raycast_ttl_s: f32,
    /// The probe footprint's neighbour spacing, in depth-buffer pixels.
    pub(crate) probe_spread_px: f32,
    /// How much nearer than the marker a sampled surface must be to count as blocking its line of
    /// sight, as a fraction of the marker's depth: a surface at the marker (the object it is
    /// attached to) does not move it.
    pub(crate) block_fraction: f32,
    /// The half-angle of the classification field of view, degrees, used when no headset frame
    /// supplies one (the flat desktop).
    pub(crate) fallback_half_fov_deg: f32,
    /// How far inside the headset's field of view a marker must be to count as on screen, degrees,
    /// so a marker at the very edge of the lenses keeps its arrow.
    pub(crate) fov_inset_deg: f32,
}

impl WorldMarkersTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive(
            "world_markers.max_distance_m",
            self.max_distance_m,
            10_000.0,
        )?;
        range(
            "world_markers.min_distance_m",
            self.min_distance_m,
            f32::EPSILON,
            self.max_distance_m,
        )?;
        nonnegative("world_markers.surface_min_m", self.surface_min_m, 100.0)?;
        nonnegative("world_markers.surface_pull_m", self.surface_pull_m, 100.0)?;
        range(
            "world_markers.surface_engage",
            self.surface_engage,
            0.0,
            1.0,
        )?;
        range(
            "world_markers.surface_release",
            self.surface_release,
            self.surface_engage,
            1.0,
        )?;
        positive(
            "world_markers.depth_halflife_s",
            self.depth_halflife_s,
            10.0,
        )?;
        positive("world_markers.forget_s", self.forget_s, 60.0)?;
        positive("world_markers.probe_ttl_s", self.probe_ttl_s, 10.0)?;
        positive("world_markers.raycast_ttl_s", self.raycast_ttl_s, 60.0)?;
        nonnegative("world_markers.probe_spread_px", self.probe_spread_px, 64.0)?;
        range(
            "world_markers.block_fraction",
            self.block_fraction,
            0.0,
            1.0,
        )?;
        positive(
            "world_markers.fallback_half_fov_deg",
            self.fallback_half_fov_deg,
            89.0,
        )?;
        nonnegative("world_markers.fov_inset_deg", self.fov_inset_deg, 45.0)
    }
}

/// The grip markers' labels (see [`crate::hud::mark_labels`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MarkLabelsTuning {
    /// A label's height, metres.
    pub height_m: f32,
    /// The gap between the marker's edge and the label's lower edge, metres.
    pub gap_m: f32,
    /// How long a published list stays current without a newer one, milliseconds.
    pub stale_ms: u64,
}

impl MarkLabelsTuning {
    /// [`MarkLabelsTuning::stale_ms`] as a duration.
    pub fn stale(&self) -> Duration {
        Duration::from_millis(self.stale_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        positive("mark_labels.height_m", self.height_m, 1.0)?;
        nonnegative("mark_labels.gap_m", self.gap_m, 1.0)?;
        millis("mark_labels.stale_ms", self.stale_ms)
    }
}

/// The markers over the weapons the player let go of (see `crate::hud::lost_weapons`).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LostWeaponsTuning {
    /// A marker is not drawn this near the eye, metres: within reach the grip's own markers show
    /// the weapon.
    pub near_m: f32,
    /// How far past [`near_m`](LostWeaponsTuning::near_m) a marker takes to fade in, metres.
    pub(crate) fade_m: f32,
    /// The most markers drawn, the nearest first.
    pub(crate) max_drawn: usize,
    /// A marker's height over its distance: its angular height, radians.
    pub(crate) angular_height: f32,
    /// How far above the weapon a marker's lower edge sits, metres.
    pub(crate) raise_m: f32,
    /// How long a published list stays current without a newer one, milliseconds.
    pub stale_ms: u64,
}

impl LostWeaponsTuning {
    /// [`LostWeaponsTuning::stale_ms`] as a duration.
    pub fn stale(&self) -> Duration {
        Duration::from_millis(self.stale_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("lost_weapons.near_m", self.near_m, 100.0)?;
        positive("lost_weapons.fade_m", self.fade_m, 100.0)?;
        if self.max_drawn > MAX_TEXTURES {
            return Err(Invalid(format!(
                "lost_weapons.max_drawn must be at most {MAX_TEXTURES}, got {}",
                self.max_drawn
            )));
        }
        positive("lost_weapons.angular_height", self.angular_height, 1.0)?;
        nonnegative("lost_weapons.raise_m", self.raise_m, 10.0)?;
        millis("lost_weapons.stale_ms", self.stale_ms)
    }
}

/// The grab hints in front of the player (see [`crate::hud::grab_hint`]). Lengths are shares of
/// the panel's height.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrabHintTuning {
    /// The height of a hint's icons' row.
    pub height: f32,
    /// How far below the panel's centre the icons' row sits, clear of the reticle.
    pub drop: f32,
    /// The least gap between two hints.
    pub gap: f32,
    /// How far from the panel's middle each of two hints is centred: about a quarter of a 16:9
    /// panel's width.
    pub spread: f32,
    /// How long a published hint stays current without a newer one, milliseconds: the input tick
    /// stopped publishing (a menu, or the weapon handling skipping its tick), so the hint no longer
    /// says what a grip does.
    pub stale_ms: u64,
    /// The hint's fade in and out, seconds.
    pub(crate) fade_s: f32,
}

impl GrabHintTuning {
    /// [`GrabHintTuning::stale_ms`] as a duration.
    pub fn stale(&self) -> Duration {
        Duration::from_millis(self.stale_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        positive("grab_hint.height", self.height, 1.0)?;
        range("grab_hint.drop", self.drop, -1.0, 1.0)?;
        nonnegative("grab_hint.gap", self.gap, 1.0)?;
        nonnegative("grab_hint.spread", self.spread, 2.0)?;
        millis("grab_hint.stale_ms", self.stale_ms)?;
        nonnegative("grab_hint.fade_s", self.fade_s, 10.0)
    }
}

/// The game's reticles' suppression (see [`crate::hud::game_reticle`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GameReticleTuning {
    /// How long the aim's results count as recent after the aim's last update, milliseconds.
    pub(crate) aim_max_age_ms: u64,
}

impl GameReticleTuning {
    /// [`GameReticleTuning::aim_max_age_ms`] as a freshness window.
    pub(crate) fn aim_window(&self) -> Window {
        Window::from_millis(self.aim_max_age_ms)
    }
}

/// The controls summary on the off-hand's wrist (see [`crate::hud::summary_panel`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryTuning {
    /// When a look at the wrist counts as a glance.
    pub glance: GlanceConfig,
    /// The panel's place on the off-hand.
    pub placement: WristPlacement,
    /// How long the panel shows after the mode changes to one the player settles in,
    /// milliseconds.
    pub(crate) show_on_mode_change_ms: u64,
    /// The fade in and out, seconds.
    pub(crate) fade_s: f32,
    /// How long each page stays while the wrist is shown, seconds.
    pub(crate) page_dwell_s: f32,
    /// How long a trackpad touch holds before it pins the panel, seconds.
    pub(crate) trackpad_pin_delay_s: f32,
}

impl SummaryTuning {
    /// [`SummaryTuning::show_on_mode_change_ms`] as a duration.
    pub(crate) fn show_on_mode_change(&self) -> Duration {
        Duration::from_millis(self.show_on_mode_change_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        validate_glance("summary.glance", &self.glance)?;
        vec3("summary.placement.offset_m", self.placement.offset_m, 1.0)?;
        positive("summary.placement.width_m", self.placement.width_m, 1.0)?;
        millis(
            "summary.show_on_mode_change_ms",
            self.show_on_mode_change_ms,
        )?;
        nonnegative("summary.fade_s", self.fade_s, 10.0)?;
        positive("summary.page_dwell_s", self.page_dwell_s, 600.0)?;
        nonnegative(
            "summary.trackpad_pin_delay_s",
            self.trackpad_pin_delay_s,
            10.0,
        )
    }
}

/// The restart notice in front of the head (see [`crate::hud::restart_notice`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestartNoticeTuning {
    /// The panel's width, metres.
    pub width_m: f32,
    /// Where the panel's centre sits in the head's frame, metres: ahead, and low enough to leave
    /// the centre of the view clear.
    pub offset_m: Vec3,
}

impl RestartNoticeTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("restart_notice.width_m", self.width_m, 10.0)?;
        vec3("restart_notice.offset_m", self.offset_m, 10.0)
    }
}

/// Check a glance's angles and times, under `path`.
fn validate_glance(path: &str, glance: &GlanceConfig) -> Result<(), Invalid> {
    nonnegative(
        &format!("{path}.view_cone_deg"),
        glance.view_cone_deg,
        180.0,
    )?;
    nonnegative(&format!("{path}.facing_deg"), glance.facing_deg, 180.0)?;
    nonnegative(
        &format!("{path}.hysteresis_deg"),
        glance.hysteresis_deg,
        90.0,
    )?;
    nonnegative(&format!("{path}.dwell_s"), glance.dwell_s, 10.0)?;
    nonnegative(&format!("{path}.linger_s"), glance.linger_s, 10.0)
}

/// The longest staleness window or repeat a value in milliseconds may give.
const MAX_MILLIS: u64 = 60_000;

/// Check that a time in milliseconds is at most [`MAX_MILLIS`].
fn millis(name: &str, value: u64) -> Result<(), Invalid> {
    if value > MAX_MILLIS {
        return Err(Invalid(format!(
            "{name} must be at most {MAX_MILLIS}, got {value}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(HUD.get().validate(), Ok(()));
    }
}
