//! The settings of grabbing a gun anywhere along it (see
//! [`crate::controllers::weapons::regions`]), held in
//! [`WeaponsConfig::regions`](crate::controllers::config::WeaponsConfig::regions).

use serde::{Deserialize, Serialize};

/// Grabbing a long gun anywhere along it. Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegionsConfig {
    /// Let a hand take a long gun, in the hands, on the body, or in the world, at whatever part
    /// of it the hand closes on.
    pub enabled: bool,
    /// How far outside the gun's model box a hand still touches the gun, metres.
    pub reach_m: f32,
    /// How far from the pistol grip a hand still holds the grip, metres.
    pub grip_radius_m: f32,
    /// How long the hand that let go of the grip for the other may keep hold of the gun where it
    /// was before its brace is given up, seconds: the brace waits for the gun to be measured in
    /// its new hand.
    pub rebrace_s: f32,
    /// How long a draw taken at a docked gun's barrel or stock waits for the gun to come into the
    /// hand, seconds.
    pub draw_wait_s: f32,
}

impl RegionsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            reach_m: 0.05,
            grip_radius_m: 0.07,
            rebrace_s: 1.0,
            draw_wait_s: 3.0,
        }
    }
}

impl Default for RegionsConfig {
    fn default() -> Self {
        Self::new()
    }
}
