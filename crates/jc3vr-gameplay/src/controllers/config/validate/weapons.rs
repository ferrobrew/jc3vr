//! Checks for physical weapon handling: the recoil strength the pause menu sets.

use crate::controllers::config::{
    WeaponsConfig,
    validate::{Invalid, nonnegative},
};

/// Check the physical weapon settings. Paths are relative to `controllers.weapons`.
pub(super) fn weapons(w: &WeaponsConfig) -> Result<(), Invalid> {
    nonnegative("weapons.recoil.kick_scale", w.recoil.kick_scale, 100.0)
}
