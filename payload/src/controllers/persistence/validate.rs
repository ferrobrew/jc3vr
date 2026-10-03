//! Range and consistency checks for a controller settings file, run before any of it is applied.

use glam::{Vec2, Vec3};

use crate::controllers::{
    config::SightTable,
    frame::PressThresholds,
    persistence::{ControllerFile, Error, FILE_VERSION},
    vehicles::config::WheelSpec,
    weapons::config::{PulseSpec, VolumeSpec},
};

/// Check every value the file carries, returning the first that is out of range or inconsistent.
pub(super) fn validate(file: &ControllerFile) -> Result<(), Error> {
    if file.version != FILE_VERSION {
        return Err(Error::UnknownVersion(u64::from(file.version)));
    }
    let c = &file.controllers;
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
        return Err(Error::Invalid(
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
    let mut hashes = [0; SightTable::CAPACITY];
    for (hash_count, sight) in c.aim.sights.entries.iter().flatten().enumerate() {
        vec3("aim.sights.offset_m", sight.offset_m, 5.0)?;
        vec2("aim.sights.angles_deg", sight.angles_deg, 360.0)?;
        if hashes[..hash_count].contains(&sight.weapon_hash) {
            return Err(Error::Invalid(
                "aim.sights contains a duplicate weapon hash".into(),
            ));
        }
        hashes[hash_count] = sight.weapon_hash;
    }
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
    volume("weapons.holsters.pouch", &w.holsters.pouch)?;
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
    pulse("weapons.haptics.grapple_fire", &w.haptics.grapple_fire)?;
    pulse("weapons.haptics.grapple_attach", &w.haptics.grapple_attach)?;
    pulse("weapons.haptics.yank", &w.haptics.yank)?;
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
    positive(
        "weapons.melee_strike.strike_speed_mps",
        w.melee_strike.strike_speed_mps,
        100.0,
    )?;
    positive(
        "weapons.melee_strike.rearm_speed_mps",
        w.melee_strike.rearm_speed_mps,
        100.0,
    )?;
    range(
        "weapons.melee_strike.min_outward",
        w.melee_strike.min_outward,
        -1.0,
        1.0,
    )?;
    nonnegative(
        "weapons.melee_strike.cooldown_s",
        w.melee_strike.cooldown_s,
        60.0,
    )?;
    pulse("weapons.melee_strike.pulse", &w.melee_strike.pulse)?;
    volume("weapons.magazine.pouch", &w.magazine.pouch)?;
    vec3(
        "weapons.magazine.well_offset_m",
        w.magazine.well_offset_m,
        5.0,
    )?;
    positive(
        "weapons.magazine.well_radius_m",
        w.magazine.well_radius_m,
        10.0,
    )?;
    positive("weapons.magazine.timeout_s", w.magazine.timeout_s, 60.0)?;
    pulse("weapons.magazine.pulse", &w.magazine.pulse)?;
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
    vec3(
        "vehicles.parachute_handles.offset_m",
        v.parachute_handles.offset_m,
        10.0,
    )?;
    positive(
        "vehicles.parachute_handles.radius_m",
        v.parachute_handles.radius_m,
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

    let turn = &file.headpose.vr_turn;
    range(
        "headpose.vr_turn.smooth_scale",
        turn.smooth_scale,
        0.0,
        10.0,
    )?;
    range(
        "headpose.vr_turn.snap_angle_deg",
        turn.snap_angle_deg,
        0.0,
        180.0,
    )?;
    range(
        "headpose.vr_turn.snap_threshold",
        turn.snap_threshold,
        0.0,
        1.0,
    )?;
    range("headpose.vr_turn.deadzone", turn.deadzone, 0.0, 1.0)?;
    range(
        "headpose.vr_turn.mouse_turn_scale",
        turn.mouse_turn_scale,
        0.0,
        100.0,
    )?;
    range(
        "headpose.vr_turn.max_body_lead_deg",
        turn.max_body_lead_deg,
        0.0,
        180.0,
    )?;
    range("headpose.vr_turn.swim_scale", turn.swim_scale, 0.0, 10.0)?;
    Ok(())
}

fn range(name: &str, value: f32, min: f32, max: f32) -> Result<(), Error> {
    if !value.is_finite() || value < min || value > max {
        return Err(Error::Invalid(format!(
            "{name} must be finite and in [{min}, {max}], got {value}"
        )));
    }
    Ok(())
}

fn press_thresholds(name: &str, thresholds: PressThresholds) -> Result<(), Error> {
    range(&format!("{name}.press"), thresholds.press, 0.0, 1.0)?;
    range(&format!("{name}.release"), thresholds.release, 0.0, 1.0)?;
    if thresholds.release > thresholds.press {
        return Err(Error::Invalid(format!("{name}.release exceeds press")));
    }
    Ok(())
}

fn positive(name: &str, value: f32, max: f32) -> Result<(), Error> {
    range(name, value, f32::EPSILON, max)
}

fn nonnegative(name: &str, value: f32, max: f32) -> Result<(), Error> {
    range(name, value, 0.0, max)
}

fn vec2(name: &str, value: Vec2, max: f32) -> Result<(), Error> {
    range(&format!("{name}.x"), value.x, -max, max)?;
    range(&format!("{name}.y"), value.y, -max, max)
}

fn vec3(name: &str, value: Vec3, max: f32) -> Result<(), Error> {
    range(&format!("{name}.x"), value.x, -max, max)?;
    range(&format!("{name}.y"), value.y, -max, max)?;
    range(&format!("{name}.z"), value.z, -max, max)
}

fn volume(name: &str, value: &VolumeSpec) -> Result<(), Error> {
    vec3(&format!("{name}.offset_m"), value.offset_m, 10.0)?;
    positive(&format!("{name}.radius_m"), value.radius_m, 10.0)
}

fn pulse(name: &str, value: &PulseSpec) -> Result<(), Error> {
    range(&format!("{name}.amplitude"), value.amplitude, 0.0, 1.0)?;
    nonnegative(&format!("{name}.duration_s"), value.duration_s, 60.0)
}

fn physical_wheel(value: &WheelSpec) -> Result<(), Error> {
    vec3("vehicles.physical.wheel.centre_m", value.centre_m, 10.0)?;
    positive("vehicles.physical.wheel.radius_m", value.radius_m, 10.0)?;
    range(
        "vehicles.physical.wheel.tilt_deg",
        value.tilt_deg,
        -180.0,
        180.0,
    )?;
    range(
        "vehicles.physical.wheel.max_angle_deg",
        value.max_angle_deg,
        0.0,
        1080.0,
    )?;
    nonnegative(
        "vehicles.physical.wheel.return_deg_per_s",
        value.return_deg_per_s,
        10_000.0,
    )
}

fn stick(name: &str, position: Vec3, deflection: f32) -> Result<(), Error> {
    vec3(name, position, 10.0)?;
    positive(&format!("{name}.max_deflection_m"), deflection, 10.0)
}

fn lever(name: &str, position: Vec3, travel: f32, detent: f32) -> Result<(), Error> {
    vec3(name, position, 10.0)?;
    positive(&format!("{name}.travel_m"), travel, 10.0)?;
    range(&format!("{name}.detent"), detent, 0.0, 1.0)
}
