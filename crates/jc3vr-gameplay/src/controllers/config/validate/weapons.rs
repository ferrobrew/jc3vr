//! Checks for physical weapon handling: the recoil strength the pause menu sets.

use jc3vr_core::config::{Invalid, nonnegative};
use jc3vr_weapons::controllers::weapons::config::WeaponsConfig;

/// Check the physical weapon settings. Paths are relative to `controllers.weapons`.
pub(super) fn weapons(w: &WeaponsConfig) -> Result<(), Invalid> {
    nonnegative("weapons.recoil.kick_scale", w.recoil.kick_scale, 100.0)
}
