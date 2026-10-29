//! Admission for HUD surfaces and panel geometry loaded from settings.

use crate::{
    config::validate::{Invalid, nonnegative, positive, range, vec3},
    hud::config::{CropExtents, HudConfig},
};

/// The Direct3D 11 two-dimensional texture axis limit.
pub(crate) const MAX_TEXTURE_DIMENSION: u32 = 16384;

pub(crate) fn hud(c: &HudConfig) -> Result<(), Invalid> {
    positive("hud_aspect", c.hud_aspect, 100.0)?;
    positive("movie_aspect", c.movie_aspect, 100.0)?;
    positive("render_scale", c.render_scale, 10.0)?;
    if let Some(resolution) = c.render_resolution {
        dimension("render_resolution", resolution)?;
    }
    positive("distance", c.distance, 1000.0)?;
    positive("panel_scale", c.panel_scale, 100.0)?;
    positive("follow.rotation_halflife", c.follow.rotation_halflife, 60.0)?;
    let e = &c.egui_panel;
    positive("egui_panel.distance", e.distance, 1000.0)?;
    positive("egui_panel.scale", e.scale, 100.0)?;
    positive("egui_panel.aspect", e.aspect, 100.0)?;
    dimension("egui_panel.resolution[0]", e.resolution.0)?;
    dimension("egui_panel.resolution[1]", e.resolution.1)?;
    nonnegative("egui_panel.click_slop", e.click_slop, 1.0)?;
    positive("egui_panel.click_duration_s", e.click_duration_s, 60.0)?;
    let d = &c.depth_shift;
    positive("depth_shift.near_threshold", d.near_threshold, 1000.0)?;
    range("depth_shift.near_occupancy", d.near_occupancy, 0.0, 1.0)?;
    range("depth_shift.hysteresis", d.hysteresis, 0.0, 1.0)?;
    positive("depth_shift.near_distance", d.near_distance, 1000.0)?;
    positive("depth_shift.halflife", d.halflife, 60.0)?;
    range("depth_shift.percentile", d.percentile, 0.0, 1.0)?;
    nonnegative("depth_shift.margin", d.margin, 1000.0)?;
    nonnegative("depth_shift.min_depth", d.min_depth, 1000.0)?;
    if !(1..=MAX_TEXTURE_DIMENSION).contains(&d.sample_stride) {
        return Err(Invalid(
            "depth_shift.sample_stride must be a positive bounded stride".into(),
        ));
    }
    positive("cursor.size", c.cursor.size, 1.0)?;
    nonnegative("cursor.lift", c.cursor.lift, 100.0)?;
    let w = &c.world_markers;
    range(
        "world_markers.fallback_half_fov_deg",
        w.fallback_half_fov_deg,
        0.1,
        89.9,
    )?;
    nonnegative("world_markers.fov_inset_deg", w.fov_inset_deg, 89.0)?;
    crop("world_markers.crop", w.crop)?;
    nonnegative(
        "world_markers.label_extra_left",
        w.label_extra_left,
        16384.0,
    )?;
    positive("world_markers.max_distance_m", w.max_distance_m, 100000.0)?;
    nonnegative("world_markers.surface_pull_m", w.surface_pull_m, 1000.0)?;
    positive("world_markers.surface_min_m", w.surface_min_m, 1000.0)?;
    nonnegative("world_markers.depth_halflife_s", w.depth_halflife_s, 60.0)?;
    let a = &c.atlas;
    nonnegative("atlas.gutter_px", a.gutter_px, 16384.0)?;
    crop("atlas.ammo.extents", a.ammo.extents)?;
    positive("atlas.ammo.max_scale", a.ammo.max_scale, 100.0)?;
    nonnegative("atlas.ammo.clearance_m", a.ammo.clearance_m, 100.0)?;
    nonnegative("atlas.ammo.pull_m", a.ammo.pull_m, 100.0)?;
    positive("atlas.ammo.width_m", a.ammo.width_m, 100.0)?;
    nonnegative(
        "atlas.ammo.follow_halflife_s",
        a.ammo.follow_halflife_s,
        60.0,
    )?;
    nonnegative("atlas.ammo.linger_s", a.ammo.linger_s, 60.0)?;
    crop("atlas.tether.extents", a.tether.extents)?;
    positive("atlas.tether.max_scale", a.tether.max_scale, 100.0)?;
    nonnegative("atlas.tether.back_m", a.tether.back_m, 100.0)?;
    nonnegative("atlas.tether.clearance_m", a.tether.clearance_m, 100.0)?;
    positive("atlas.tether.width_m", a.tether.width_m, 100.0)?;
    nonnegative(
        "atlas.tether.follow_halflife_s",
        a.tether.follow_halflife_s,
        60.0,
    )?;
    nonnegative("atlas.cross.margin_px", a.cross.margin_px, 16384.0)?;
    positive("atlas.cross.max_scale", a.cross.max_scale, 100.0)?;
    positive("atlas.cross.width_m", a.cross.width_m, 100.0)?;
    nonnegative("atlas.cross.pull_m", a.cross.pull_m, 100.0)?;
    let wrist = &a.wrist;
    crop("atlas.wrist.heat_extents", wrist.heat_extents)?;
    positive("atlas.wrist.heat_max_scale", wrist.heat_max_scale, 100.0)?;
    vec3("atlas.wrist.offset_m", wrist.offset_m, 100.0)?;
    positive("atlas.wrist.width_m", wrist.width_m, 100.0)?;
    positive("atlas.wrist.bar_aspect", wrist.bar_aspect, 100.0)?;
    nonnegative("atlas.wrist.fade_s", wrist.fade_s, 60.0)?;
    nonnegative("atlas.wrist.after_damage_s", wrist.after_damage_s, 60.0)?;
    range("atlas.wrist.low_health", wrist.low_health, 0.0, 1.0)?;
    range(
        "atlas.wrist.glance.view_cone_deg",
        wrist.glance.view_cone_deg,
        0.0,
        180.0,
    )?;
    range(
        "atlas.wrist.glance.facing_deg",
        wrist.glance.facing_deg,
        0.0,
        180.0,
    )?;
    range(
        "atlas.wrist.glance.hysteresis_deg",
        wrist.glance.hysteresis_deg,
        0.0,
        180.0,
    )?;
    nonnegative("atlas.wrist.glance.dwell_s", wrist.glance.dwell_s, 60.0)?;
    nonnegative("atlas.wrist.glance.linger_s", wrist.glance.linger_s, 60.0)
}

fn crop(name: &str, c: CropExtents) -> Result<(), Invalid> {
    for (side, value) in [
        ("left", c.left),
        ("right", c.right),
        ("up", c.up),
        ("down", c.down),
    ] {
        nonnegative(&format!("{name}.{side}"), value, 16384.0)?;
    }
    positive(name, c.left + c.right, 32768.0)?;
    positive(name, c.up + c.down, 32768.0)
}

pub(crate) fn dimension(name: &str, value: u32) -> Result<(), Invalid> {
    if !(1..=MAX_TEXTURE_DIMENSION).contains(&value) {
        return Err(Invalid(format!(
            "{name} must be between 1 and {MAX_TEXTURE_DIMENSION}, got {value}"
        )));
    }
    Ok(())
}
