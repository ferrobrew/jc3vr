//! Range and consistency checks for the controller settings whose types admit values the
//! subsystems cannot use, the per-weapon sight corrections among them. The tolerant load
//! ([`jc3vr_core::config::load`]) runs them on a loaded configuration through the controller
//! settings' slot ([`reset_invalid`]), and resets the setting a failure names, or the subtree when
//! that does not make it pass. A failure is an [`Invalid`], which the engine owns so that a
//! subsystem's own checks need nothing from this module.
//!
//! The checks are split by config area, one module per subsystem (or the group's own `validate`,
//! where the group's crate owns its checks), each returning the first out-of-range or inconsistent
//! setting in `controllers`' order. They are built from the engine's range checks
//! ([`jc3vr_core::config::range`] and its specializations).

mod aim;
mod haptics;
mod input;
mod physical_hands;
mod weapons;

use jc3vr_body::controllers::aim::sights::config::SightTable;
use jc3vr_core::config::{
    Invalid,
    load::{Reset, reset_named},
    vec2, vec3,
};

use crate::controllers::config::ControllersConfig;

/// Check every controller setting except the sight corrections ([`sights`]), returning the first
/// that is out of range or inconsistent. Paths are relative to `controllers`.
pub fn controllers(c: &ControllersConfig) -> Result<(), Invalid> {
    c.body.rig.validate()?;
    c.body.ik.validate()?;
    input::bindings(&c.bindings)?;
    aim::hands(&c.hands)?;
    aim::aim(&c.aim)?;
    weapons::weapons(&c.weapons)?;
    c.vehicles.validate()?;
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

/// Reset the controller settings at `path` that fail their checks. The checks name the setting
/// that fails, so that setting is reset alone (or the group around it, when its default does not
/// pass either), and the settings are reset whole only when that does not make them pass. The
/// sight corrections are measured rather than chosen, so a failure elsewhere keeps them.
pub(crate) fn reset_invalid(c: &mut ControllersConfig, path: &str, resets: &mut Vec<Reset>) {
    if let Err(error) = sights(&c.aim.sights) {
        c.aim.sights = SightTable::EMPTY;
        resets.push(Reset {
            path: format!("{path}.aim.sights"),
            reason: error.to_string(),
        });
    }
    if let Err(error) = reset_named(c, &ControllersConfig::new(), path, controllers, resets) {
        let sights = c.aim.sights;
        *c = ControllersConfig::new();
        c.aim.sights = sights;
        resets.push(Reset {
            path: path.to_owned(),
            reason: error.to_string(),
        });
    }
}
