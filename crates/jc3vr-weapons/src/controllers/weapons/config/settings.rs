//! The settings one weapon-handling tick reads, gathered from their groups.

use jc3vr_body::controllers::{
    aim::config::{AIM_CONFIG, AimConfig},
    scheme,
};
use jc3vr_input::frame::Hand;

use crate::controllers::weapons::config::{WEAPONS_CONFIG, WeaponsConfig};

/// The settings the weapon handling reads in one tick: its own group, and the fields of the lower
/// groups it consults. [`WeaponSettings::read`] takes them one group at a time, so no two
/// configuration guards are held at once, and the tick then reads them all from this one copy.
#[derive(Clone)]
pub struct WeaponSettings {
    /// The controllers' master switch ([`scheme::enabled`]).
    pub enabled: bool,
    /// The hand the roles follow ([`scheme::acting_hand`]).
    pub dominant: Hand,
    /// Physical weapon handling.
    pub weapons: WeaponsConfig,
    /// Per-hand aiming, whose sight table places a weapon's sight line.
    pub aim: AimConfig,
}

impl WeaponSettings {
    /// The settings as they are now.
    pub fn read() -> Self {
        let enabled = scheme::enabled();
        let dominant = scheme::acting_hand();
        let weapons = *WEAPONS_CONFIG.lock();
        let aim = *AIM_CONFIG.lock();
        Self {
            enabled,
            dominant,
            weapons,
            aim,
        }
    }

    /// The defaults of every group, with the right hand dominant: the settings a fresh install
    /// reads with no controller bindings attached.
    pub const fn new() -> Self {
        Self {
            enabled: true,
            dominant: Hand::Right,
            weapons: WeaponsConfig::new(),
            aim: AimConfig::new(),
        }
    }
}

impl Default for WeaponSettings {
    fn default() -> Self {
        Self::new()
    }
}
