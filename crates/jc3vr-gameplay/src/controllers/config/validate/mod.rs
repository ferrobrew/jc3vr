//! Range and consistency checks for the settings whose types admit values the subsystems cannot
//! use: the controller tuning, the per-weapon sight corrections, and the VR turn settings. The
//! payload's tolerant load ([`jc3vr_core::config::engine::load`]) runs them on a loaded
//! configuration, through the configuration's schema, and resets the setting a failure names, or
//! the subtree when that does not make it pass. A failure is an
//! [`Invalid`](jc3vr_core::config::engine::Invalid), which the engine owns so that a subsystem's own
//! checks need nothing from this module.
//!
//! The checks are split by config area, one module per subsystem, each returning the first
//! out-of-range or inconsistent setting in `controllers`' order. They are built from the engine's
//! range checks ([`jc3vr_core::config::engine::range`] and its specializations).

mod aim;
mod haptics;
mod input;
mod physical_hands;
mod roomscale;
mod vehicles;
mod weapons;

use jc3vr_core::config::engine::{Invalid, range, vec2, vec3};
use jc3vr_render::headpose::config::VrTurnConfig;

use crate::controllers::{
    config::{ControllersConfig, SightTable},
    locomotion::config::validate as locomotion,
};

/// Check every controller setting except the sight corrections ([`sights`]), returning the first
/// that is out of range or inconsistent. Paths are relative to `controllers`.
pub fn controllers(c: &ControllersConfig) -> Result<(), Invalid> {
    locomotion(&c.locomotion.stick)?;
    c.body.procedural.validate()?;
    c.body.rig.validate()?;
    c.body.estimator.validate()?;
    c.body.ik.validate()?;
    c.input.validate()?;
    input::bindings(&c.bindings)?;
    aim::hands(&c.hands)?;
    aim::aim(&c.aim)?;
    aim::overlay(&c.overlay)?;
    weapons::weapons(&c.weapons)?;
    roomscale::roomscale(&c.roomscale)?;
    vehicles::vehicles(&c.vehicles)?;
    physical_hands::physical_hands(&c.physical_hands)?;
    haptics::haptics(&c.haptics)
}

/// Check the per-weapon sight corrections: finite, in range, and at most one per weapon.
pub fn sights(sights: &SightTable) -> Result<(), Invalid> {
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
pub fn vr_turn(turn: &VrTurnConfig) -> Result<(), Invalid> {
    range("smooth_scale", turn.smooth_scale, 0.0, 10.0)?;
    range("snap_angle_deg", turn.snap_angle_deg, 0.0, 180.0)?;
    range("snap_threshold", turn.snap_threshold, 0.0, 1.0)?;
    range("deadzone", turn.deadzone, 0.0, 1.0)?;
    Ok(())
}
