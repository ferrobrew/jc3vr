//! The stunt positions' settings.

use serde::{Deserialize, Serialize};

/// Deliberate roof-mantle entry and grip-open exit from the game's existing helicopter hang
/// (see [`crate::controllers::physical_hands::stunt`]). Physical grips retain their handholds
/// through the hand climb instead of requesting the animated hang.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StuntConfig {
    /// Hold stunt positions with the grip.
    pub enabled: bool,
    /// Enter a vehicle's stunt area when a mantle lands on its roof.
    pub enter_roof: bool,
    /// How far from the nearest stunt area the feet may land for the mantle to enter it, metres.
    pub roof_offer_m: f32,
    /// Let go of the helicopter when both grips let go in its hang, once a grip has held there.
    pub let_go: bool,
    /// How long both grips must stay open in the hang before it lets go, seconds: a hand moved
    /// from one grip to the other does not.
    pub release_grace_s: f32,
    /// How long to wait after an entry the game refused before trying again, seconds.
    pub retry_s: f32,
    /// The helicopter's speed of the strongest hold pulse, metres per second.
    pub hold_full_speed_mps: f32,
    /// The diameter of the bar the fingers close round on a helicopter's skid, metres.
    pub skid_diameter_m: f32,
}

impl StuntConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            enter_roof: true,
            roof_offer_m: 0.6,
            let_go: true,
            release_grace_s: 0.2,
            retry_s: 1.0,
            hold_full_speed_mps: 40.0,
            skid_diameter_m: 0.05,
        }
    }
}

impl Default for StuntConfig {
    fn default() -> Self {
        Self::new()
    }
}
