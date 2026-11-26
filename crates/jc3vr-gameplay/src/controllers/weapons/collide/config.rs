//! The settings of held guns against walls (see [`crate::controllers::weapons::collide`]), held
//! in [`WeaponsConfig::collide`](crate::controllers::config::WeaponsConfig::collide).

use serde::{Deserialize, Serialize};

/// Held long guns stop at walls. Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CollideConfig {
    /// Push a held long gun out of the walls and objects it would pass into.
    pub enabled: bool,
}

impl CollideConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for CollideConfig {
    fn default() -> Self {
        Self::new()
    }
}
