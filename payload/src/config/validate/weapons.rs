//! Checks for physical weapon handling: holsters, pouches, the foregrip, recoil, heft,
//! the grenade and C4 throws, the scope, and the true reload.

use crate::{
    config::validate::{Invalid, nonnegative, positive, range, vec3, volume},
    controllers::{
        config::WeaponsConfig,
        weapons::{config::HeftConfig, reload::config::ReloadConfig},
    },
};

/// Check the physical weapon settings. Paths are relative to `controllers.weapons`.
pub(super) fn weapons(w: &WeaponsConfig) -> Result<(), Invalid> {
    volume("weapons.holsters.hip", &w.holsters.hip)?;
    volume("weapons.holsters.shoulder", &w.holsters.shoulder)?;
    volume("weapons.holsters.chest", &w.holsters.chest)?;
    volume("weapons.pouches.grenade", &w.pouches.grenade)?;
    volume("weapons.pouches.c4", &w.pouches.c4)?;
    volume("weapons.pouches.magazine", &w.pouches.magazine)?;
    nonnegative(
        "weapons.pouches.reach_margin_m",
        w.pouches.reach_margin_m,
        1.0,
    )?;
    nonnegative(
        "weapons.holsters.crouch_deadband_m",
        w.holsters.crouch_deadband_m,
        10.0,
    )?;
    positive("weapons.foregrip.radius_m", w.foregrip.radius_m, 10.0)?;
    positive(
        "weapons.foregrip.release_distance_m",
        w.foregrip.release_distance_m,
        10.0,
    )?;
    nonnegative("weapons.foregrip.blend_s", w.foregrip.blend_s, 60.0)?;
    positive(
        "weapons.foregrip.fallback_distance_m",
        w.foregrip.fallback_distance_m,
        10.0,
    )?;
    nonnegative("weapons.foregrip.min_span_m", w.foregrip.min_span_m, 10.0)?;
    nonnegative("weapons.recoil.kick_scale", w.recoil.kick_scale, 100.0)?;
    nonnegative(
        "weapons.recoil.kick_back_m_per_rad",
        w.recoil.kick_back_m_per_rad,
        100.0,
    )?;
    range(
        "weapons.recoil.max_angle_deg",
        w.recoil.max_angle_deg,
        0.0,
        360.0,
    )?;
    nonnegative(
        "weapons.recoil.spring_stiffness",
        w.recoil.spring_stiffness,
        100_000.0,
    )?;
    range(
        "weapons.recoil.spring_damping_ratio",
        w.recoil.spring_damping_ratio,
        0.0,
        10.0,
    )?;
    nonnegative(
        "weapons.recoil.two_hand_stiffness_scale",
        w.recoil.two_hand_stiffness_scale,
        100.0,
    )?;
    nonnegative(
        "weapons.recoil.two_hand_kick_scale",
        w.recoil.two_hand_kick_scale,
        100.0,
    )?;
    range(
        "weapons.foregrip.max_turn_deg",
        w.foregrip.max_turn_deg,
        0.0,
        180.0,
    )?;
    heft(&w.heft)?;
    vec3(
        "weapons.grenade.hold_offset_m",
        w.grenade.hold_offset_m,
        5.0,
    )?;
    positive(
        "weapons.grenade.release_window_s",
        w.grenade.release_window_s,
        60.0,
    )?;
    nonnegative(
        "weapons.grenade.release_peak_span_s",
        w.grenade.release_peak_span_s,
        60.0,
    )?;
    nonnegative(
        "weapons.grenade.velocity_scale",
        w.grenade.velocity_scale,
        100.0,
    )?;
    nonnegative(
        "weapons.grenade.max_speed_mps",
        w.grenade.max_speed_mps,
        1_000.0,
    )?;
    nonnegative("weapons.grenade.warning_s", w.grenade.warning_s, 60.0)?;
    for (index, value) in w.grenade.warning_interval_s.into_iter().enumerate() {
        positive(
            &format!("weapons.grenade.warning_interval_s[{index}]"),
            value,
            60.0,
        )?;
    }
    let c4 = &w.c4;
    vec3("weapons.c4.hold_offset_m", c4.hold_offset_m, 5.0)?;
    positive("weapons.c4.release_window_s", c4.release_window_s, 60.0)?;
    nonnegative(
        "weapons.c4.release_peak_span_s",
        c4.release_peak_span_s,
        60.0,
    )?;
    nonnegative("weapons.c4.velocity_scale", c4.velocity_scale, 100.0)?;
    nonnegative("weapons.c4.max_speed_mps", c4.max_speed_mps, 1_000.0)?;
    let d = &c4.detonator;
    vec3("weapons.c4.detonator.offset_m", d.offset_m, 5.0)?;
    vec3("weapons.c4.detonator.normal", d.normal, 5.0)?;
    positive("weapons.c4.detonator.radius_m", d.radius_m, 1.0)?;
    positive("weapons.c4.detonator.touch_radius_m", d.touch_radius_m, 1.0)?;
    range(
        "weapons.c4.detonator.release_radius_m",
        d.release_radius_m,
        d.touch_radius_m,
        1.0,
    )?;
    nonnegative("weapons.c4.detonator.max_behind_m", d.max_behind_m, 1.0)?;
    nonnegative(
        "weapons.c4.detonator.min_entry_speed_mps",
        d.min_entry_speed_mps,
        100.0,
    )?;
    nonnegative("weapons.c4.detonator.dwell_s", d.dwell_s, 60.0)?;
    vec3(
        "weapons.c4.detonator.fingertip_pointing_m",
        d.fingertip_pointing_m,
        5.0,
    )?;
    vec3(
        "weapons.c4.detonator.fingertip_resting_m",
        d.fingertip_resting_m,
        5.0,
    )?;
    reload(&w.reload)?;
    positive("weapons.scope.zoom", w.scope.zoom, 100.0)?;
    positive("weapons.scope.lens_radius_m", w.scope.lens_radius_m, 10.0)?;
    range(
        "weapons.scope.eyepiece_back_m",
        w.scope.eyepiece_back_m,
        -10.0,
        10.0,
    )?;
    positive(
        "weapons.scope.max_eye_distance_m",
        w.scope.max_eye_distance_m,
        10.0,
    )?;
    range(
        "weapons.scope.vignette_start",
        w.scope.vignette_start,
        0.0,
        1.0,
    )
}

fn heft(h: &HeftConfig) -> Result<(), Invalid> {
    nonnegative("weapons.heft.reach_near_m", h.reach_near_m, 10.0)?;
    nonnegative("weapons.heft.reach_far_m", h.reach_far_m, 10.0)?;
    if h.reach_far_m < h.reach_near_m {
        return Err(Invalid(format!(
            "weapons.heft.reach_far_m ({}) must not be below weapons.heft.reach_near_m ({})",
            h.reach_far_m, h.reach_near_m
        )));
    }
    nonnegative("weapons.heft.sway_hz", h.sway_hz, 10.0)?;
    range("weapons.heft.max_angle_deg", h.max_angle_deg, 0.0, 180.0)?;
    for (name, t) in [
        ("weapons.heft.sidearm", &h.sidearm),
        ("weapons.heft.rifle", &h.rifle),
        ("weapons.heft.heavy", &h.heavy),
    ] {
        range(&format!("{name}.sag_deg"), t.sag_deg, 0.0, 90.0)?;
        range(&format!("{name}.reach_sag_deg"), t.reach_sag_deg, 0.0, 90.0)?;
        nonnegative(&format!("{name}.lag_stiffness"), t.lag_stiffness, 100_000.0)?;
        range(
            &format!("{name}.lag_damping_ratio"),
            t.lag_damping_ratio,
            0.0,
            10.0,
        )?;
        range(&format!("{name}.sway_deg"), t.sway_deg, 0.0, 45.0)?;
        nonnegative(&format!("{name}.kick_scale"), t.kick_scale, 100.0)?;
        nonnegative(&format!("{name}.spread_scale"), t.spread_scale, 100.0)?;
    }
    let b = &h.braced;
    range("weapons.heft.braced.strength", b.strength, 0.0, 10.0)?;
    range(
        "weapons.heft.braced.max_angle_deg",
        b.max_angle_deg,
        0.0,
        90.0,
    )?;
    for (name, t) in [
        ("weapons.heft.braced.smg", &b.smg),
        ("weapons.heft.braced.rifle", &b.rifle),
        ("weapons.heft.braced.heavy", &b.heavy),
    ] {
        range(&format!("{name}.sag_deg"), t.sag_deg, 0.0, 90.0)?;
        nonnegative(&format!("{name}.stiffness"), t.stiffness, 100_000.0)?;
    }
    Ok(())
}

fn reload(r: &ReloadConfig) -> Result<(), Invalid> {
    range("weapons.reload.tap_s", r.tap_s, 0.0, 5.0)?;
    positive(
        "weapons.reload.flick_speed_dps",
        r.flick_speed_dps,
        10_000.0,
    )?;
    nonnegative("weapons.reload.flick_cooldown_s", r.flick_cooldown_s, 60.0)?;
    vec3("weapons.reload.well_offset_m", r.well_offset_m, 5.0)?;
    nonnegative("weapons.reload.held_top_m", r.held_top_m, 1.0)?;
    positive("weapons.reload.capture_radius_m", r.capture_radius_m, 1.0)?;
    range(
        "weapons.reload.capture_angle_deg",
        r.capture_angle_deg,
        0.0,
        180.0,
    )?;
    positive("weapons.reload.seat_depth_m", r.seat_depth_m, 1.0)?;
    positive("weapons.reload.timeout_s", r.timeout_s, 600.0)?;
    range(
        "weapons.reload.belt.max_tilt_deg",
        r.belt.max_tilt_deg,
        0.0,
        180.0,
    )?;
    nonnegative(
        "weapons.reload.belt.min_push_speed_mps",
        r.belt.min_push_speed_mps,
        20.0,
    )?;
    nonnegative("weapons.reload.belt.depth_m", r.belt.depth_m, 1.0)?;
    nonnegative(
        "weapons.reload.belt.after_draw_s",
        r.belt.after_draw_s,
        60.0,
    )?;
    positive(
        "weapons.reload.action.grab_radius_m",
        r.action.grab_radius_m,
        1.0,
    )?;
    range(
        "weapons.reload.action.pull_fraction",
        r.action.pull_fraction,
        0.0,
        1.0,
    )?;
    range(
        "weapons.reload.action.return_fraction",
        r.action.return_fraction,
        0.0,
        r.action.pull_fraction,
    )?;
    positive(
        "weapons.reload.action.release_distance_m",
        r.action.release_distance_m,
        5.0,
    )?;
    positive(
        "weapons.reload.action.default_travel_m",
        r.action.default_travel_m,
        1.0,
    )?;
    positive("weapons.reload.action.spring_s", r.action.spring_s, 5.0)?;
    range(
        "weapons.reload.action.bolt_lift_deg",
        r.action.bolt_lift_deg,
        1.0,
        180.0,
    )?;
    positive(
        "weapons.reload.action.bolt_radius_m",
        r.action.bolt_radius_m,
        0.5,
    )?;
    range(
        "weapons.reload.action.lift_fraction",
        r.action.lift_fraction,
        0.5,
        1.0,
    )?;
    Ok(())
}
