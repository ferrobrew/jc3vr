//! Range and consistency checks for the settings whose types admit values the subsystems cannot
//! use: the controller tuning, the per-weapon sight corrections, and the VR turn settings. The
//! tolerant load ([`crate::config::load`]) runs them on a loaded configuration and resets the
//! setting a failure names, or the subtree when that does not make it pass.
//!
//! The checks are split by config area, one module per subsystem, each returning the first
//! out-of-range or inconsistent setting in `controllers`' order. The primitives below (`range` and
//! its specializations) are shared by every area.

mod aim;
mod fingers;
mod input;
mod physical_hands;
mod roomscale;
mod vehicles;
mod weapons;

use std::fmt;

use glam::{Vec2, Vec3};

use crate::{
    controllers::{
        config::{ControllersConfig, SightTable},
        frame::PressThresholds,
        locomotion::config::validate as locomotion,
        weapons::config::{PulseSpec, VolumeSpec},
    },
    headpose::config::VrTurnConfig,
};

/// A setting that is well-typed but out of range or inconsistent with another. The message leads
/// with the setting's path, relative to the checked subtree, which the tolerant load resets.
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
    input::input(&c.input)?;
    input::bindings(&c.bindings)?;
    input::mode(&c.mode)?;
    aim::aim_state(&c.aim_state)?;
    aim::hands(&c.hands)?;
    aim::aim(&c.aim)?;
    aim::overlay(&c.overlay)?;
    weapons::weapons(&c.weapons)?;
    roomscale::roomscale(&c.roomscale)?;
    vehicles::vehicles(&c.vehicles)?;
    fingers::fingers(&c.fingers)?;
    physical_hands::physical_hands(&c.physical_hands)
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

fn press_thresholds(name: &str, thresholds: PressThresholds) -> Result<(), Invalid> {
    range(&format!("{name}.press"), thresholds.press, 0.0, 1.0)?;
    range(&format!("{name}.release"), thresholds.release, 0.0, 1.0)?;
    if thresholds.release > thresholds.press {
        return Err(Invalid(format!("{name}.release exceeds press")));
    }
    Ok(())
}
