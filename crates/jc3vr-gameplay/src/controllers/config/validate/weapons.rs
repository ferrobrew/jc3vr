//! Checks for physical weapon handling: the recoil strength the pause menu sets.

use jc3vr_core::config::engine::{Invalid, nonnegative};

use crate::controllers::config::WeaponsConfig;

/// Check the physical weapon settings. Paths are relative to `controllers.weapons`.
pub(super) fn weapons(w: &WeaponsConfig) -> Result<(), Invalid> {
    nonnegative("weapons.recoil.kick_scale", w.recoil.kick_scale, 100.0)
}
