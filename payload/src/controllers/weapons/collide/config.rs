//! The settings of held guns against walls (see [`crate::controllers::weapons::collide`]), held
//! in [`WeaponsConfig::collide`](crate::controllers::config::WeaponsConfig::collide).

use serde::{Deserialize, Serialize};

/// Held long guns stop at walls. Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CollideConfig {
    /// Push a held long gun out of the walls and objects it would pass into.
    pub enabled: bool,
    /// How far past the muzzle the gun's probe reaches, metres, so the muzzle stops just short of
    /// a wall.
    pub tip_margin_m: f32,
    /// The furthest the gun turns away from a wall about its grip, degrees.
    pub max_turn_deg: f32,
    /// The furthest the gun is pulled back into the hands, metres.
    pub max_retract_m: f32,
    /// How long a wall is kept after the probe last met it, seconds.
    pub keep_s: f32,
}

impl CollideConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            tip_margin_m: 0.03,
            max_turn_deg: 70.0,
            max_retract_m: 0.35,
            keep_s: 0.15,
        }
    }
}

impl Default for CollideConfig {
    fn default() -> Self {
        Self::new()
    }
}
