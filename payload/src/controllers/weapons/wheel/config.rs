//! The weapon wheel's settings, held in
//! [`WeaponsConfig::wheel`](crate::controllers::config::WeaponsConfig::wheel). The fixed tuning is
//! [`WHEEL`](crate::controllers::weapons::wheel::tuning::WHEEL).

use serde::{Deserialize, Serialize};

/// The weapon wheel (see [`crate::controllers::weapons::wheel`]). It acts only under the controller
/// scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WheelConfig {
    /// Open the game's selection cross with a held input and choose an arm with the hand.
    pub enabled: bool,
}

impl WheelConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for WheelConfig {
    fn default() -> Self {
        Self::new()
    }
}
