//! Range and consistency checks for the settings whose types admit values the subsystems cannot
//! use: the controller tuning, the per-weapon sight corrections, and the VR turn settings. The
//! tolerant load ([`crate::config::load`]) runs them on a loaded configuration and resets a subtree
//! that fails to its defaults.

use std::fmt;

use glam::{Vec2, Vec3};

use crate::{
    controllers::{
        config::{ControllersConfig, SightTable},
        frame::PressThresholds,
        locomotion::config::validate as locomotion,
        physical_hands::{
            PhysicalHandsConfig,
            config::{PickupConfig, ShoveConfig},
            physics::HandBodiesConfig,
        },
        vehicles::config::WheelSpec,
        weapons::{
            config::{HeftConfig, PulseSpec, VolumeSpec},
            reload::config::ReloadConfig,
        },
    },
    headpose::config::VrTurnConfig,
};

/// A setting that is well-typed but out of range or inconsistent with another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invalid(pub String);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid setting: {}", self.0)
    }
}

impl std::error::Error for Invalid {}

/// Check every controller setting except the sight corrections ([`sights`]), returning the first
/// that is out of range or inconsistent. Paths are relative to `controllers`.
pub(crate) fn controllers(c: &ControllersConfig) -> Result<(), Invalid> {
    locomotion(&c.locomotion.stick, &c.locomotion.acts, &c.locomotion.view)?;
    c.body.procedural.validate()?;
    let input = &c.input;
    press_thresholds("input.trigger_press", input.trigger_press)?;
    press_thresholds("input.squeeze_press", input.squeeze_press)?;
    press_thresholds("input.squeeze_force_press", input.squeeze_force_press)?;
    press_thresholds("input.trackpad_press", input.trackpad_press)?;
    range("input.stick_deadzone", input.stick_deadzone, 0.0, 1.0)?;

    range("bindings.move_deadzone", c.bindings.move_deadzone, 0.0, 1.0)?;
    range(
        "bindings.trigger_deadzone",
        c.bindings.trigger_deadzone,
        0.0,
        1.0,
    )?;
    positive("bindings.long_press_s", c.bindings.long_press_s, 60.0)?;
    range("bindings.evade_flick", c.bindings.evade_flick, 0.0, 1.0)?;
    positive("bindings.evade_window_s", c.bindings.evade_window_s, 60.0)?;
    range("bindings.crouch_flick", c.bindings.crouch_flick, 0.0, 1.0)?;
    range(
        "bindings.menu_stick_push",
        c.bindings.menu_stick_push,
        0.0,
        1.0,
    )?;
    nonnegative(
        "bindings.stick_chord_window_s",
        c.bindings.stick_chord_window_s,
        1.0,
    )?;
    positive("bindings.recenter_hold_s", c.bindings.recenter_hold_s, 60.0)?;
    nonnegative(
        "bindings.panel_chord_window_s",
        c.bindings.panel_chord_window_s,
        1.0,
    )?;
    positive(
        "bindings.yank.min_distance_m",
        c.bindings.yank.min_distance_m,
        10.0,
    )?;
    positive(
        "bindings.yank.min_speed_mps",
        c.bindings.yank.min_speed_mps,
        100.0,
    )?;
    positive("bindings.yank.window_s", c.bindings.yank.window_s, 60.0)?;
    nonnegative("bindings.yank.cooldown_s", c.bindings.yank.cooldown_s, 60.0)?;
    nonnegative(
        "bindings.yank.body_drop_m",
        c.bindings.yank.body_drop_m,
        10.0,
    )?;
    if c.mode.debounce_ticks == 0 || c.mode.debounce_ticks > 60 {
        return Err(Invalid(
            "mode.debounce_ticks must be between 1 and 60".into(),
        ));
    }

    nonnegative(
        "aim_state.neutral_aim_distance_m",
        c.aim_state.neutral_aim_distance_m,
        10_000.0,
    )?;
    range("hands.reach_t", c.hands.reach_t, 0.0, 1.0)?;
    range("hands.reach_r", c.hands.reach_r, 0.0, 1.0)?;
    range("hands.pull", c.hands.pull, 0.0, 1.0)?;
    range("hands.resist", c.hands.resist, 0.0, 1.0)?;
    nonnegative("hands.yield_blend_s", c.hands.yield_blend_s, 60.0)?;
    nonnegative("hands.return_blend_s", c.hands.return_blend_s, 60.0)?;
    vec3("hands.grip.position_m", c.hands.grip.position_m, 5.0)?;
    vec3("hands.grip.rotation_deg", c.hands.grip.rotation_deg, 360.0)?;
    nonnegative("hands.anchor.blend_s", c.hands.anchor.blend_s, 60.0)?;
    nonnegative(
        "hands.anchor.swing_deadzone_m",
        c.hands.anchor.swing_deadzone_m,
        10.0,
    )?;
    positive(
        "hands.anchor.swing_full_m",
        c.hands.anchor.swing_full_m,
        10.0,
    )?;
    range(
        "hands.anchor.swing_scale",
        c.hands.anchor.swing_scale,
        -100.0,
        100.0,
    )?;
    let reach = &c.hands.reach;
    nonnegative(
        "hands.reach.shoulder_smoothing_s",
        reach.shoulder_smoothing_s,
        60.0,
    )?;
    nonnegative("hands.reach.pull_start", reach.pull_start, 10.0)?;
    nonnegative("hands.reach.pull_full", reach.pull_full, 10.0)?;
    range("hands.reach.pull_max", reach.pull_max, 0.0, 1.0)?;
    nonnegative(
        "hands.reach.detach_threshold_m",
        reach.detach_threshold_m,
        1.0,
    )?;
    nonnegative("hands.reach.detach_cap_m", reach.detach_cap_m, 1.0)?;

    range("aim.assist_strength", c.aim.assist_strength, 0.0, 1.0)?;
    nonnegative(
        "aim.assist_max_distance_m",
        c.aim.assist_max_distance_m,
        10_000.0,
    )?;
    nonnegative(
        "aim.assist_occlusion_tolerance_m",
        c.aim.assist_occlusion_tolerance_m,
        100.0,
    )?;
    positive("aim.max_distance_m", c.aim.max_distance_m, 100_000.0)?;
    nonnegative(
        "aim.min_target_distance_m",
        c.aim.min_target_distance_m,
        100.0,
    )?;
    vec3("aim.grapple_origin_m", c.aim.grapple_origin_m, 5.0)?;
    nonnegative(
        "aim.sight_tuning.marker_distance_m",
        c.aim.sight_tuning.marker_distance_m,
        10_000.0,
    )?;

    nonnegative(
        "overlay.depth_tolerance_m",
        c.overlay.depth_tolerance_m,
        100.0,
    )?;
    nonnegative(
        "overlay.depth_tolerance_rel",
        c.overlay.depth_tolerance_rel,
        100.0,
    )?;
    positive("overlay.min_line_px", c.overlay.min_line_px, 10_000.0)?;
    range("overlay.xray_alpha", c.overlay.xray_alpha, 0.0, 1.0)?;

    let w = &c.weapons;
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
    pulse("weapons.haptics.grapple_fire", &w.haptics.grapple_fire)?;
    pulse("weapons.haptics.grapple_attach", &w.haptics.grapple_attach)?;
    pulse("weapons.haptics.yank", &w.haptics.yank)?;
    pulse("weapons.haptics.dry_fire", &w.haptics.dry_fire)?;
    range("weapons.haptics.intensity", w.haptics.intensity, 0.0, 1.0)?;
    nonnegative(
        "weapons.haptics.kick_amplitude_per_rad",
        w.haptics.kick_amplitude_per_rad,
        100.0,
    )?;
    nonnegative("weapons.haptics.max_pulse_s", w.haptics.max_pulse_s, 60.0)?;
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
    pulse("weapons.grenade.warning_pulse", &w.grenade.warning_pulse)?;
    for (index, value) in w.grenade.warning_interval_s.into_iter().enumerate() {
        positive(
            &format!("weapons.grenade.warning_interval_s[{index}]"),
            value,
            60.0,
        )?;
    }
    pulse("weapons.grenade.prime_pulse", &w.grenade.prime_pulse)?;
    pulse("weapons.grenade.refused_pulse", &w.grenade.refused_pulse)?;
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
    pulse("weapons.c4.prime_pulse", &c4.prime_pulse)?;
    pulse("weapons.c4.throw_pulse", &c4.throw_pulse)?;
    pulse("weapons.c4.refused_pulse", &c4.refused_pulse)?;
    pulse("weapons.c4.detonate_pulse", &c4.detonate_pulse)?;
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
    pulse("weapons.c4.detonator.pulse", &d.pulse)?;
    pulse("weapons.c4.detonator.empty_pulse", &d.empty_pulse)?;
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
    )?;

    let r = &c.roomscale;
    positive(
        "roomscale.roomscale_min_extent_m",
        r.roomscale_min_extent_m,
        100.0,
    )?;
    nonnegative(
        "roomscale.auto_posture.stand_after_s",
        r.auto_posture.stand_after_s,
        600.0,
    )?;
    nonnegative(
        "roomscale.auto_posture.sit_after_s",
        r.auto_posture.sit_after_s,
        3_600.0,
    )?;
    range(
        "roomscale.seated_reference.window_s",
        r.seated_reference.window_s,
        10.0,
        3_600.0,
    )?;
    nonnegative(
        "roomscale.seated_reference.rate_mps",
        r.seated_reference.rate_mps,
        1.0,
    )?;
    nonnegative(
        "roomscale.seated_reference.deadband_m",
        r.seated_reference.deadband_m,
        1.0,
    )?;
    nonnegative("roomscale.chase.deadzone_m", r.chase.deadzone_m, 100.0)?;
    nonnegative(
        "roomscale.chase.max_speed_mps",
        r.chase.max_speed_mps,
        1_000.0,
    )?;
    positive(
        "roomscale.chase.time_constant_s",
        r.chase.time_constant_s,
        60.0,
    )?;
    nonnegative("roomscale.chase.warp_min_m", r.chase.warp_min_m, 1_000.0)?;
    range("roomscale.crouch.hips_share", r.crouch.hips_share, 0.0, 1.0)?;
    nonnegative("roomscale.crouch.enter_drop_m", r.crouch.enter_drop_m, 10.0)?;
    nonnegative("roomscale.crouch.exit_drop_m", r.crouch.exit_drop_m, 10.0)?;
    nonnegative(
        "roomscale.crouch.seated_drop_m",
        r.crouch.seated_drop_m,
        10.0,
    )?;
    nonnegative(
        "roomscale.head_fade.start_depth_m",
        r.head_fade.start_depth_m,
        100.0,
    )?;
    positive(
        "roomscale.head_fade.full_depth_m",
        r.head_fade.full_depth_m,
        100.0,
    )?;
    nonnegative("roomscale.fade.out_s", r.fade.out_s, 60.0)?;
    nonnegative("roomscale.fade.in_s", r.fade.in_s, 60.0)?;
    nonnegative("roomscale.seat_fade_hold_s", r.seat_fade_hold_s, 60.0)?;

    let v = &c.vehicles;
    positive("vehicles.mounted.min_lever_m", v.mounted.min_lever_m, 10.0)?;
    physical_wheel(&v.physical.wheel)?;
    vec3(
        "vehicles.physical.handlebars.centre_m",
        v.physical.handlebars.centre_m,
        10.0,
    )?;
    positive(
        "vehicles.physical.handlebars.half_width_m",
        v.physical.handlebars.half_width_m,
        10.0,
    )?;
    range(
        "vehicles.physical.handlebars.max_steer_deg",
        v.physical.handlebars.max_steer_deg,
        0.0,
        360.0,
    )?;
    range(
        "vehicles.physical.handlebars.max_lean_deg",
        v.physical.handlebars.max_lean_deg,
        0.0,
        360.0,
    )?;
    stick(
        "vehicles.physical.cyclic.top_m",
        v.physical.cyclic.top_m,
        v.physical.cyclic.max_deflection_m,
    )?;
    lever(
        "vehicles.physical.collective.rest_m",
        v.physical.collective.rest_m,
        v.physical.collective.travel_m,
        v.physical.collective.detent,
    )?;
    stick(
        "vehicles.physical.flight_stick.top_m",
        v.physical.flight_stick.top_m,
        v.physical.flight_stick.max_deflection_m,
    )?;
    lever(
        "vehicles.physical.throttle.rest_m",
        v.physical.throttle.rest_m,
        v.physical.throttle.travel_m,
        v.physical.throttle.detent,
    )?;
    positive(
        "vehicles.physical.grab_radius_m",
        v.physical.grab_radius_m,
        10.0,
    )?;
    positive(
        "vehicles.physical.release_distance_m",
        v.physical.release_distance_m,
        10.0,
    )?;
    range(
        "vehicles.arm_steering.min_spread",
        v.arm_steering.min_spread,
        0.0,
        2.0,
    )?;
    nonnegative(
        "vehicles.arm_steering.deadzone_m",
        v.arm_steering.deadzone_m,
        10.0,
    )?;
    positive(
        "vehicles.arm_steering.full_bank_m",
        v.arm_steering.full_bank_m,
        10.0,
    )?;
    positive(
        "vehicles.arm_steering.full_pitch_m",
        v.arm_steering.full_pitch_m,
        10.0,
    )?;
    range(
        "vehicles.arm_steering.full_tilt_deg",
        v.arm_steering.full_tilt_deg,
        1.0,
        180.0,
    )?;
    range(
        "vehicles.arm_steering.tilt_deadzone_deg",
        v.arm_steering.tilt_deadzone_deg,
        0.0,
        90.0,
    )?;
    range(
        "vehicles.arm_steering.tilt_neutral_deg",
        v.arm_steering.tilt_neutral_deg,
        -90.0,
        90.0,
    )?;
    positive(
        "vehicles.arm_steering.full_sweep_m",
        v.arm_steering.full_sweep_m,
        10.0,
    )?;
    vec3(
        "vehicles.driving_view.hub_from_eyes_m",
        v.driving_view.hub_from_eyes_m,
        2.0,
    )?;
    nonnegative("vehicles.driving_view.max_m", v.driving_view.max_m, 1.0)?;
    nonnegative(
        "vehicles.driving_view.blend_s",
        v.driving_view.blend_s,
        10.0,
    )?;
    vec3(
        "vehicles.parachute_handles.from_shoulder_m",
        v.parachute_handles.from_shoulder_m,
        10.0,
    )?;
    positive(
        "vehicles.parachute_handles.radius_m",
        v.parachute_handles.radius_m,
        10.0,
    )?;
    positive(
        "vehicles.parachute_handles.reach_m",
        v.parachute_handles.reach_m,
        10.0,
    )?;
    positive(
        "vehicles.parachute_handles.full_pull_m",
        v.parachute_handles.full_pull_m,
        10.0,
    )?;
    nonnegative(
        "vehicles.parachute_handles.deadzone_m",
        v.parachute_handles.deadzone_m,
        10.0,
    )?;
    range(
        "vehicles.parachute_handles.brake_threshold",
        v.parachute_handles.brake_threshold,
        0.0,
        1.0,
    )?;
    positive(
        "vehicles.parachute_handles.release_distance_m",
        v.parachute_handles.release_distance_m,
        10.0,
    )?;

    for (index, finger_range) in c.fingers.curl_ranges.into_iter().enumerate() {
        range(
            &format!("fingers.curl_ranges[{index}].open"),
            finger_range.open,
            -2.0,
            2.0,
        )?;
        range(
            &format!("fingers.curl_ranges[{index}].closed"),
            finger_range.closed,
            -2.0,
            2.0,
        )?;
    }
    for (index, value) in c.fingers.finger_bend_deg.into_iter().enumerate() {
        range(
            &format!("fingers.finger_bend_deg[{index}]"),
            value,
            0.0,
            360.0,
        )?;
    }
    for (index, value) in c.fingers.thumb_bend_deg.into_iter().enumerate() {
        range(
            &format!("fingers.thumb_bend_deg[{index}]"),
            value,
            0.0,
            360.0,
        )?;
    }
    for (finger, offsets) in c.fingers.rest_offset_deg.into_iter().enumerate() {
        for (joint, value) in offsets.into_iter().enumerate() {
            range(
                &format!("fingers.rest_offset_deg[{finger}][{joint}]"),
                value,
                -180.0,
                180.0,
            )?;
        }
    }
    range(
        "fingers.thumb_spread_deg",
        c.fingers.thumb_spread_deg,
        -180.0,
        180.0,
    )?;
    nonnegative("fingers.splay_scale", c.fingers.splay_scale, 10.0)?;
    range("fingers.max_splay_deg", c.fingers.max_splay_deg, 0.0, 180.0)?;
    nonnegative("fingers.smoothing_s", c.fingers.smoothing_s, 60.0)?;
    physical_hands(&c.physical_hands)
}

/// Check the per-weapon sight corrections: finite, in range, and at most one per weapon.
pub(crate) fn sights(sights: &SightTable) -> Result<(), Invalid> {
    let mut hashes = [0; SightTable::CAPACITY];
    for (hash_count, sight) in sights.entries.iter().flatten().enumerate() {
        vec3("aim.sights.offset_m", sight.offset_m, 5.0)?;
        vec2("aim.sights.angles_deg", sight.angles_deg, 360.0)?;
        if hashes[..hash_count].contains(&sight.weapon_hash) {
            return Err(Invalid(
                "aim.sights contains a duplicate weapon hash".into(),
            ));
        }
        hashes[hash_count] = sight.weapon_hash;
    }
    Ok(())
}

/// Check the VR turn settings. Paths are relative to `headpose.vr_turn`.
pub(crate) fn vr_turn(turn: &VrTurnConfig) -> Result<(), Invalid> {
    range("smooth_scale", turn.smooth_scale, 0.0, 10.0)?;
    range("snap_angle_deg", turn.snap_angle_deg, 0.0, 180.0)?;
    range("snap_threshold", turn.snap_threshold, 0.0, 1.0)?;
    range("deadzone", turn.deadzone, 0.0, 1.0)?;
    range("mouse_turn_scale", turn.mouse_turn_scale, 0.0, 100.0)?;
    range("max_body_lead_deg", turn.max_body_lead_deg, 0.0, 180.0)?;
    range("swim_scale", turn.swim_scale, 0.0, 10.0)?;
    Ok(())
}

fn range(name: &str, value: f32, min: f32, max: f32) -> Result<(), Invalid> {
    if !value.is_finite() || value < min || value > max {
        return Err(Invalid(format!(
            "{name} must be finite and in [{min}, {max}], got {value}"
        )));
    }
    Ok(())
}

fn press_thresholds(name: &str, thresholds: PressThresholds) -> Result<(), Invalid> {
    range(&format!("{name}.press"), thresholds.press, 0.0, 1.0)?;
    range(&format!("{name}.release"), thresholds.release, 0.0, 1.0)?;
    if thresholds.release > thresholds.press {
        return Err(Invalid(format!("{name}.release exceeds press")));
    }
    Ok(())
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

fn physical_hands(p: &PhysicalHandsConfig) -> Result<(), Invalid> {
    for (axis, value) in ["x", "y", "z"]
        .into_iter()
        .zip(p.palm_half_extents_m.to_array())
    {
        positive(
            &format!("physical_hands.palm_half_extents_m.{axis}"),
            value,
            0.5,
        )?;
    }
    vec3("physical_hands.palm_offset_m", p.palm_offset_m, 0.5)?;
    nonnegative("physical_hands.push_deadband_m", p.push_deadband_m, 1.0)?;
    nonnegative(
        "physical_hands.push_stiffness_per_s",
        p.push_stiffness_per_s,
        1000.0,
    )?;
    nonnegative(
        "physical_hands.max_push_speed_mps",
        p.max_push_speed_mps,
        20.0,
    )?;
    range(
        "physical_hands.min_push_horizontal",
        p.min_push_horizontal,
        0.0,
        1.0,
    )?;
    positive(
        "physical_hands.full_hit_speed_mps",
        p.full_hit_speed_mps,
        50.0,
    )?;
    nonnegative(
        "physical_hands.min_object_speed_mps",
        p.min_object_speed_mps,
        50.0,
    )?;
    nonnegative("physical_hands.object_impulse", p.object_impulse, 100.0)?;
    pulse("physical_hands.contact_pulse", &p.contact_pulse)?;
    pulse("physical_hands.pressure_pulse", &p.pressure_pulse)?;
    shove(&p.shove)?;
    pickup(&p.pickup)?;
    hand_bodies(&p.bodies)
}

fn shove(s: &ShoveConfig) -> Result<(), Invalid> {
    nonnegative(
        "physical_hands.shove.stiffness_per_s",
        s.stiffness_per_s,
        1000.0,
    )?;
    nonnegative("physical_hands.shove.deadband_m", s.deadband_m, 1.0)?;
    nonnegative("physical_hands.shove.max_speed_mps", s.max_speed_mps, 50.0)?;
    nonnegative("physical_hands.shove.max_force_n", s.max_force_n, 100_000.0)?;
    nonnegative("physical_hands.shove.friction", s.friction, 10.0)?;
    nonnegative(
        "physical_hands.shove.give_smoothing_s",
        s.give_smoothing_s,
        10.0,
    )
}

fn pickup(p: &PickupConfig) -> Result<(), Invalid> {
    nonnegative("physical_hands.pickup.reach_m", p.reach_m, 1.0)?;
    nonnegative(
        "physical_hands.pickup.max_mass_kg",
        p.max_mass_kg,
        100_000.0,
    )?;
    nonnegative(
        "physical_hands.pickup.hand_force_n",
        p.hand_force_n,
        100_000.0,
    )?;
    nonnegative(
        "physical_hands.pickup.hand_torque_nm",
        p.hand_torque_nm,
        10_000.0,
    )?;
    range(
        "physical_hands.pickup.two_hand_angular_share",
        p.two_hand_angular_share,
        0.0,
        1.0,
    )?;
    nonnegative("physical_hands.pickup.gravity_mps2", p.gravity_mps2, 100.0)?;
    nonnegative(
        "physical_hands.pickup.keep_out_radius_m",
        p.keep_out_radius_m,
        5.0,
    )?;
    nonnegative(
        "physical_hands.pickup.stiffness_per_s",
        p.stiffness_per_s,
        1000.0,
    )?;
    nonnegative(
        "physical_hands.pickup.max_speed_mps",
        p.max_speed_mps,
        200.0,
    )?;
    nonnegative(
        "physical_hands.pickup.angular_stiffness_per_s",
        p.angular_stiffness_per_s,
        1000.0,
    )?;
    nonnegative(
        "physical_hands.pickup.max_angular_speed_rps",
        p.max_angular_speed_rps,
        200.0,
    )?;
    positive(
        "physical_hands.pickup.break_distance_m",
        p.break_distance_m,
        100.0,
    )?;
    nonnegative("physical_hands.pickup.throw_scale", p.throw_scale, 20.0)?;
    nonnegative(
        "physical_hands.pickup.max_throw_speed_mps",
        p.max_throw_speed_mps,
        500.0,
    )?;
    pulse("physical_hands.pickup.grab_pulse", &p.grab_pulse)?;
    pulse("physical_hands.pickup.release_pulse", &p.release_pulse)
}

fn hand_bodies(b: &HandBodiesConfig) -> Result<(), Invalid> {
    range("physical_hands.bodies.strength", b.strength, 0.0, 10.0)?;
    positive("physical_hands.bodies.mass_kg", b.mass_kg, 1000.0)?;
    nonnegative(
        "physical_hands.bodies.max_force_n",
        b.max_force_n,
        1_000_000.0,
    )?;
    nonnegative(
        "physical_hands.bodies.max_torque_nm",
        b.max_torque_nm,
        100_000.0,
    )?;
    range("physical_hands.bodies.linear_gain", b.linear_gain, 0.0, 1.0)?;
    range(
        "physical_hands.bodies.angular_gain",
        b.angular_gain,
        0.0,
        1.0,
    )?;
    nonnegative(
        "physical_hands.bodies.max_speed_mps",
        b.max_speed_mps,
        200.0,
    )?;
    nonnegative(
        "physical_hands.bodies.max_angular_speed_rps",
        b.max_angular_speed_rps,
        500.0,
    )?;
    positive("physical_hands.bodies.radius_m", b.radius_m, 0.5)?;
    nonnegative("physical_hands.bodies.half_length_m", b.half_length_m, 0.5)?;
    positive(
        "physical_hands.bodies.break_distance_m",
        b.break_distance_m,
        10.0,
    )?;
    positive(
        "physical_hands.bodies.body_mass_kg",
        b.body_mass_kg,
        10_000.0,
    )?;
    nonnegative(
        "physical_hands.bodies.push_deadband_m",
        b.push_deadband_m,
        1.0,
    )?;
    nonnegative(
        "physical_hands.bodies.max_push_speed_mps",
        b.max_push_speed_mps,
        20.0,
    )?;
    range(
        "physical_hands.bodies.min_push_horizontal",
        b.min_push_horizontal,
        0.0,
        1.0,
    )?;
    nonnegative(
        "physical_hands.bodies.push_smoothing_s",
        b.push_smoothing_s,
        10.0,
    )?;
    positive(
        "physical_hands.bodies.pressure_full_lag_m",
        b.pressure_full_lag_m,
        2.0,
    )
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
    let h = &r.haptics;
    for (name, spec) in [
        ("weapons.reload.haptics.eject", &h.eject),
        ("weapons.reload.haptics.pouch_take", &h.pouch_take),
        ("weapons.reload.haptics.pouch_empty", &h.pouch_empty),
        ("weapons.reload.haptics.capture", &h.capture),
        ("weapons.reload.haptics.seat", &h.seat),
        ("weapons.reload.haptics.belt_seat", &h.belt_seat),
        ("weapons.reload.haptics.rack", &h.rack),
        ("weapons.reload.haptics.chambered", &h.chambered),
    ] {
        pulse(name, spec)?;
    }
    Ok(())
}

fn positive(name: &str, value: f32, max: f32) -> Result<(), Invalid> {
    range(name, value, f32::EPSILON, max)
}

fn nonnegative(name: &str, value: f32, max: f32) -> Result<(), Invalid> {
    range(name, value, 0.0, max)
}

fn vec2(name: &str, value: Vec2, max: f32) -> Result<(), Invalid> {
    range(&format!("{name}.x"), value.x, -max, max)?;
    range(&format!("{name}.y"), value.y, -max, max)
}

fn vec3(name: &str, value: Vec3, max: f32) -> Result<(), Invalid> {
    range(&format!("{name}.x"), value.x, -max, max)?;
    range(&format!("{name}.y"), value.y, -max, max)?;
    range(&format!("{name}.z"), value.z, -max, max)
}

fn volume(name: &str, value: &VolumeSpec) -> Result<(), Invalid> {
    vec3(&format!("{name}.offset_m"), value.offset_m, 10.0)?;
    positive(&format!("{name}.radius_m"), value.radius_m, 10.0)
}

fn pulse(name: &str, value: &PulseSpec) -> Result<(), Invalid> {
    range(&format!("{name}.amplitude"), value.amplitude, 0.0, 1.0)?;
    nonnegative(&format!("{name}.duration_s"), value.duration_s, 60.0)
}

fn physical_wheel(value: &WheelSpec) -> Result<(), Invalid> {
    vec3("vehicles.physical.wheel.centre_m", value.centre_m, 10.0)?;
    positive("vehicles.physical.wheel.radius_m", value.radius_m, 10.0)?;
    range(
        "vehicles.physical.wheel.tilt_deg",
        value.tilt_deg,
        -180.0,
        180.0,
    )?;
    range(
        "vehicles.physical.wheel.lock_deg",
        value.lock_deg,
        1.0,
        1080.0,
    )?;
    range("vehicles.physical.wheel.response", value.response, 0.2, 5.0)?;
    range(
        "vehicles.physical.wheel.deadzone_deg",
        value.deadzone_deg,
        0.0,
        90.0,
    )?;
    nonnegative(
        "vehicles.physical.wheel.return_deg_per_s",
        value.return_deg_per_s,
        10_000.0,
    )
}

fn stick(name: &str, position: Vec3, deflection: f32) -> Result<(), Invalid> {
    vec3(name, position, 10.0)?;
    positive(&format!("{name}.max_deflection_m"), deflection, 10.0)
}

fn lever(name: &str, position: Vec3, travel: f32, detent: f32) -> Result<(), Invalid> {
    vec3(name, position, 10.0)?;
    positive(&format!("{name}.travel_m"), travel, 10.0)?;
    range(&format!("{name}.detent"), detent, 0.0, 1.0)
}
