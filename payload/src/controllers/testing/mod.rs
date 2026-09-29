//! Persistent controller tuning, validation, and named switch presets.
//!
//! The controller file is intentionally separate from the runtime configuration type. Loading is
//! explicit, and the complete file is parsed and validated before either the controller settings or
//! the shared VR turn settings are changed. Player body dimensions remain owned by `body.local.json`.

use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
};

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::{
    config::Config, controllers::config::ControllersConfig, headpose::config::VrTurnConfig,
};

/// The file written beside the payload DLL.
pub const FILE_NAME: &str = "controllers.local.json";
/// The current on-disk format version.
pub const FILE_VERSION: u32 = 1;

mod presets;
mod worker;
pub use presets::{Preset, apply_preset, preset_modified};
pub use worker::shutdown;
pub(crate) use worker::{Request, Status, queue, start_pending, status};

/// The versioned controller settings file.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerFile {
    /// The file format version, not the payload version.
    pub version: u32,
    /// The controller settings, including tuning and sight calibration.
    pub controllers: ControllersConfig,
    /// Settings shared with the headpose subsystem.
    pub headpose: HeadposeFile,
}

/// The part of the headpose configuration owned by this file.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadposeFile {
    /// The VR body-turn settings also used by the controller right stick.
    pub vr_turn: VrTurnConfig,
}

/// Why a controller settings operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The payload path is not available yet.
    PathUnavailable,
    /// A file operation failed.
    Io {
        operation: &'static str,
        detail: String,
    },
    /// JSON could not be parsed or serialized.
    Json(String),
    /// The file has a version this payload does not understand.
    UnknownVersion(u32),
    /// A value is syntactically valid but unsafe or inconsistent.
    Invalid(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PathUnavailable => write!(f, "controller settings: the payload path is unknown"),
            Self::Io { operation, detail } => {
                write!(f, "controller settings: could not {operation}: {detail}")
            }
            Self::Json(detail) => write!(f, "controller settings: invalid JSON: {detail}"),
            Self::UnknownVersion(version) => write!(
                f,
                "controller settings: unsupported file version {version} (expected {FILE_VERSION})"
            ),
            Self::Invalid(detail) => write!(f, "controller settings: invalid value: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

/// The path used by the explicit save and load operations.
pub fn file_path() -> Option<PathBuf> {
    Some(crate::module::get_path()?.parent()?.join(FILE_NAME))
}

/// Build the persisted representation from the runtime configuration.
pub fn from_config(config: &Config) -> ControllerFile {
    ControllerFile {
        version: FILE_VERSION,
        controllers: config.controllers,
        headpose: HeadposeFile {
            vr_turn: config.headpose.vr_turn,
        },
    }
}

/// Parse, validate, and return a controller settings file without changing runtime state.
pub fn parse(text: &str) -> Result<ControllerFile, Error> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| Error::Json(e.to_string()))?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            Error::Json("the top-level `version` is missing or is not an integer".into())
        })?;
    let version = u32::try_from(version)
        .map_err(|_| Error::Json("the top-level `version` is too large".into()))?;
    if version != FILE_VERSION {
        return Err(Error::UnknownVersion(version));
    }
    let file: ControllerFile =
        serde_json::from_value(value.clone()).map_err(|e| Error::Json(e.to_string()))?;
    let expected = serde_json::to_value(file).map_err(|e| Error::Json(e.to_string()))?;
    check_shape(&value, &expected, "settings")?;
    validate(&file)?;
    Ok(file)
}

/// Apply a previously parsed file after validating it again at the mutation boundary.
pub fn apply(file: &ControllerFile, config: &mut Config) -> Result<(), Error> {
    validate(file)?;
    config.controllers = file.controllers;
    config.headpose.vr_turn = file.headpose.vr_turn;
    Ok(())
}

/// Save an immutable snapshot without accessing the live configuration.
pub fn save_snapshot_to_path(path: &Path, file: &ControllerFile) -> Result<PathBuf, Error> {
    validate(file)?;
    let json = serde_json::to_string_pretty(file).map_err(|e| Error::Json(e.to_string()))?;
    atomic_replace(path, json.as_bytes())?;
    Ok(path.to_owned())
}

fn check_shape(
    actual: &serde_json::Value,
    expected: &serde_json::Value,
    path: &str,
) -> Result<(), Error> {
    match (actual, expected) {
        (serde_json::Value::Object(a), serde_json::Value::Object(e)) => {
            for key in a.keys() {
                if !e.contains_key(key) {
                    return Err(Error::Json(format!("unknown field {path}.{key}")));
                }
            }
            for (key, value) in e {
                let actual = a
                    .get(key)
                    .ok_or_else(|| Error::Json(format!("missing field {path}.{key}")))?;
                check_shape(actual, value, &format!("{path}.{key}"))?;
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(e)) => {
            if a.len() != e.len() {
                return Err(Error::Json(format!("wrong array length at {path}")));
            }
            for (index, (a, e)) in a.iter().zip(e).enumerate() {
                check_shape(a, e, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate(file: &ControllerFile) -> Result<(), Error> {
    if file.version != FILE_VERSION {
        return Err(Error::UnknownVersion(file.version));
    }
    let c = &file.controllers;
    let input = &c.input;
    range(
        "input.trigger_press.press",
        input.trigger_press.press,
        0.0,
        1.0,
    )?;
    range(
        "input.trigger_press.release",
        input.trigger_press.release,
        0.0,
        1.0,
    )?;
    if input.trigger_press.release > input.trigger_press.press {
        return Err(Error::Invalid(
            "input.trigger_press.release exceeds press".into(),
        ));
    }
    range(
        "input.squeeze_press.press",
        input.squeeze_press.press,
        0.0,
        1.0,
    )?;
    range(
        "input.squeeze_press.release",
        input.squeeze_press.release,
        0.0,
        1.0,
    )?;
    if input.squeeze_press.release > input.squeeze_press.press {
        return Err(Error::Invalid(
            "input.squeeze_press.release exceeds press".into(),
        ));
    }
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
    let mut hashes = [0; crate::controllers::config::SightTable::CAPACITY];
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

    for (index, finger_range) in c.fingers.ranges.into_iter().enumerate() {
        range(
            &format!("fingers.ranges[{index}].open"),
            finger_range.open,
            -2.0,
            2.0,
        )?;
        range(
            &format!("fingers.ranges[{index}].closed"),
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

fn volume(
    name: &str,
    value: &crate::controllers::weapons::config::VolumeSpec,
) -> Result<(), Error> {
    vec3(&format!("{name}.offset_m"), value.offset_m, 10.0)?;
    positive(&format!("{name}.radius_m"), value.radius_m, 10.0)
}

fn pulse(name: &str, value: &crate::controllers::weapons::config::PulseSpec) -> Result<(), Error> {
    range(&format!("{name}.amplitude"), value.amplitude, 0.0, 1.0)?;
    nonnegative(&format!("{name}.duration_s"), value.duration_s, 60.0)
}

fn physical_wheel(value: &crate::controllers::vehicles::config::WheelSpec) -> Result<(), Error> {
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

fn io_error(operation: &'static str, error: std::io::Error) -> Error {
    Error::Io {
        operation,
        detail: error.to_string(),
    }
}

static TEMP_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn atomic_replace(path: &Path, contents: &[u8]) -> Result<(), Error> {
    let parent = path.parent().ok_or(Error::PathUnavailable)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::Io {
            operation: "name the settings file",
            detail: "the path has no valid file name".into(),
        })?;
    let (temporary, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error("create the temporary settings file", error)),
        }
    };
    let result = (|| {
        file.write_all(contents)
            .map_err(|e| io_error("write the temporary settings file", e))?;
        file.sync_all()
            .map_err(|e| io_error("flush the temporary settings file", e))?;
        drop(file);
        replace_path(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn replace_path(temporary: &Path, destination: &Path) -> Result<(), Error> {
    use std::{iter, os::windows::ffi::OsStrExt};

    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };

    let source: Vec<u16> = temporary
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let target: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(target.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|e| {
        io_error(
            "replace the settings file",
            std::io::Error::other(e.to_string()),
        )
    })
}

#[cfg(not(windows))]
fn replace_path(temporary: &Path, destination: &Path) -> Result<(), Error> {
    fs::rename(temporary, destination).map_err(|e| io_error("replace the settings file", e))
}

#[cfg(test)]
mod review_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{config::SchemeSetting, frame::Hand};

    fn config() -> Config {
        Config::new()
    }

    #[test]
    fn the_versioned_format_round_trips_a_real_controller_file() {
        let mut original = config();
        original.controllers.dominant_hand = Hand::Left;
        original.controllers.hands.grip.position_m = Vec3::new(0.1, -0.2, 0.3);
        original
            .controllers
            .aim
            .sights
            .set(crate::controllers::config::SightOverride {
                weapon_hash: 42,
                offset_m: Vec3::new(0.0, 0.03, 0.0),
                angles_deg: Vec2::new(1.0, -2.0),
            });
        original.headpose.vr_turn.smooth_scale = 1.75;
        let file = from_config(&original);
        let text = serde_json::to_string(&file).unwrap();
        let parsed = parse(&text).unwrap();
        let mut loaded = config();
        apply(&parsed, &mut loaded).unwrap();
        assert_eq!(loaded.controllers.dominant_hand, Hand::Left);
        assert_eq!(
            loaded.controllers.hands.grip.position_m,
            Vec3::new(0.1, -0.2, 0.3)
        );
        assert_eq!(
            loaded.controllers.aim.sights.get(42).unwrap().angles_deg,
            Vec2::new(1.0, -2.0)
        );
        assert_eq!(loaded.headpose.vr_turn.smooth_scale, 1.75);
    }

    #[test]
    fn an_invalid_file_does_not_partially_apply() {
        let mut loaded = config();
        let before = loaded.controllers.hands.grip.position_m;
        let mut file = from_config(&loaded);
        file.controllers.hands.grip.position_m.x = 0.25;
        file.controllers.aim.grapple_origin_m.x = f32::NAN;
        assert!(apply(&file, &mut loaded).is_err());
        assert_eq!(loaded.controllers.hands.grip.position_m, before);
    }

    #[test]
    fn presets_preserve_tuning_and_calibration() {
        let mut config = config();
        config.controllers.hands.grip.position_m = Vec3::new(0.12, -0.08, 0.04);
        config
            .controllers
            .aim
            .sights
            .set(crate::controllers::config::SightOverride {
                weapon_hash: 99,
                offset_m: Vec3::new(0.0, 0.02, -0.01),
                angles_deg: Vec2::new(2.0, 3.0),
            });
        config.headpose.vr_turn.snap_angle_deg = 47.0;
        let grip = config.controllers.hands.grip;
        let sight = config.controllers.aim.sights.get(99).copied();
        let turn = config.headpose.vr_turn.snap_angle_deg;
        apply_preset(&mut config.controllers, Preset::VehiclesAndAdvanced);
        assert_eq!(config.controllers.hands.grip, grip);
        assert_eq!(config.controllers.aim.sights.get(99).copied(), sight);
        assert_eq!(config.headpose.vr_turn.snap_angle_deg, turn);
    }

    #[test]
    fn preset_modified_reports_switch_differences_without_tuning_differences() {
        let mut config = config();
        apply_preset(&mut config.controllers, Preset::VehiclesAndAdvanced);
        assert!(!preset_modified(
            &config.controllers,
            Preset::VehiclesAndAdvanced
        ));
        config.controllers.hands.grip.position_m.x += 0.1;
        assert!(!preset_modified(
            &config.controllers,
            Preset::VehiclesAndAdvanced
        ));
        config.controllers.vehicles.physical.wheel.enabled = false;
        assert!(preset_modified(
            &config.controllers,
            Preset::VehiclesAndAdvanced
        ));
    }

    #[test]
    fn gamepad_baseline_turns_off_controller_and_roomscale_features() {
        let mut config = config();
        apply_preset(&mut config.controllers, Preset::GamepadBaseline);
        assert!(!config.controllers.enabled);
        assert_eq!(config.controllers.scheme, SchemeSetting::Gamepad);
        assert!(!config.controllers.roomscale.chase.enabled);
        assert!(!config.controllers.roomscale.crouch.visual);
        assert!(!config.controllers.roomscale.crouch.capsule);
        assert!(!config.controllers.roomscale.head_fade.enabled);
        assert!(!config.controllers.body.remap_head);
        assert!(!config.controllers.body.remap_hands);
    }

    #[test]
    fn unknown_versions_are_rejected_before_deserialization() {
        let text = r#"{"version":99,"controllers":{},"headpose":{}}"#;
        assert!(matches!(parse(text), Err(Error::UnknownVersion(99))));
    }
}
