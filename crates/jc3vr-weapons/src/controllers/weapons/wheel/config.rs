//! The weapon wheel's settings, held in
//! [`WeaponsConfig::wheel`](crate::controllers::weapons::config::WeaponsConfig::wheel). The tuning
//! is [`WheelTuning`](crate::controllers::weapons::wheel::tuning::WheelTuning).

use serde::{Deserialize, Serialize};

/// The weapon wheel (see [`crate::controllers::weapons::wheel`]). It defaults off: one shared cross
/// opened from the panel breaks the body-held weapons the hands draw from the sockets, which would
/// take an independent wheel for each hand.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WheelConfig {
    /// Open the game's selection cross with a held input and choose an arm with the hand.
    pub enabled: bool,
}

impl WheelConfig {
    pub const fn new() -> Self {
        Self { enabled: false }
    }
}

impl Default for WheelConfig {
    fn default() -> Self {
        Self::new()
    }
}
