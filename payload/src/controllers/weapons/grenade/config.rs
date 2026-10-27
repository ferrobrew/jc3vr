//! The settings for catching live grenades, held in
//! [`GrenadeConfig`](crate::controllers::weapons::config::GrenadeConfig) alongside the pouch throw's
//! own tuning.

use serde::{Deserialize, Serialize};

/// Catching a live grenade near the player and throwing it on. Acts only under the controller
/// scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrenadeCatchConfig {
    /// Let a free hand catch a live grenade with a grip press at it.
    pub enabled: bool,
    /// The radius of a live grenade's grab volume, metres. A grenade in flight moves several
    /// centimetres between two ticks, so it is larger than a pouch's.
    pub radius_m: f32,
    /// How far from the player a live grenade may be to be caught, metres.
    pub range_m: f32,
    /// How long after the player's own throw the grenade cannot be caught, seconds, so that the
    /// hand that threw it does not take it back as it leaves.
    pub own_grace_s: f32,
    /// The fastest a caught grenade is moved toward the hand, metres per second.
    pub max_carry_mps: f32,
}

impl GrenadeCatchConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            radius_m: 0.18,
            range_m: 4.0,
            own_grace_s: 0.4,
            max_carry_mps: 25.0,
        }
    }
}

impl Default for GrenadeCatchConfig {
    fn default() -> Self {
        Self::new()
    }
}
