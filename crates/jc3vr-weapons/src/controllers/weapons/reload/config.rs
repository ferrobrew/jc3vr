//! The true reload's settings (see [`crate::controllers::weapons::reload`]), held in
//! [`WeaponsConfig::reload`](crate::controllers::weapons::config::WeaponsConfig::reload). The fixed tuning
//! is [`RELOAD`](crate::controllers::weapons::reload::tuning::RELOAD).

use serde::{Deserialize, Serialize};

/// The true reload. Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReloadConfig {
    /// Reload by hand: the game's own reloads of the weapons the mod reloads are stopped, and a
    /// magazine is ejected, taken from the belt pouch, and seated in the well by hand, which
    /// chambers its first round.
    /// Off, the game's timed reload plays from the reload button as it does on a gamepad.
    pub enabled: bool,
}

impl ReloadConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for ReloadConfig {
    fn default() -> Self {
        Self::new()
    }
}
