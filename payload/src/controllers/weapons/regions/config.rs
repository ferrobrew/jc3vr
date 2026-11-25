//! The settings of grabbing a gun anywhere along it (see
//! [`crate::controllers::weapons::regions`]), held in
//! [`WeaponsConfig::regions`](crate::controllers::config::WeaponsConfig::regions).

use serde::{Deserialize, Serialize};

/// Grabbing a long gun anywhere along it. Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegionsConfig {
    /// Let a second hand brace a long gun at whatever part of it the hand closes on. A lone hand
    /// takes a gun by its grip wherever it touches it, whether this is on or off.
    pub enabled: bool,
}

impl RegionsConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for RegionsConfig {
    fn default() -> Self {
        Self::new()
    }
}
