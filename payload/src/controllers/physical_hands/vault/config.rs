//! The push-up's settings.

use serde::{Deserialize, Serialize};

/// Pushing the body up with the hands (see [`crate::controllers::physical_hands::vault`]): a hand
/// body pressed down on a surface lifts the player, a jump assist bounded by how far the hand
/// presses and how high the player has risen.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultConfig {
    pub enabled: bool,
    /// The downward press left alone before it lifts, metres of the player's hand, so that a hand
    /// resting on a table lifts nothing.
    pub deadband_m: f32,
    /// The lifting speed per metre of press past the deadband, per second.
    pub stiffness_per_s: f32,
    /// The fastest lift, metres per second.
    pub max_lift_mps: f32,
    /// The least downward share of a press for it to lift, `0..=1`.
    pub min_vertical: f32,
    /// The highest the feet rise above where the push started, metres.
    pub max_rise_m: f32,
}

impl VaultConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            deadband_m: 0.04,
            stiffness_per_s: 10.0,
            max_lift_mps: 3.0,
            min_vertical: 0.7,
            max_rise_m: 1.4,
        }
    }
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self::new()
    }
}
