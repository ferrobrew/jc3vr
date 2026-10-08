//! The stunt positions' settings.

use serde::{Deserialize, Serialize};

/// Stunt positions held by the player's grip (see
/// [`crate::controllers::physical_hands::stunt`]): a helicopter gripped by the hands takes the
/// player into its hang, a mantle onto a vehicle's roof takes the player into its stunt area,
/// and letting go of the grips in the hang lets go of the helicopter. It needs the hand climbing,
/// whose grips on moving bodies it builds on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StuntConfig {
    /// Hold stunt positions with the grip.
    pub enabled: bool,
    /// Enter a helicopter's hang when a hand grips it and the body hangs below the grip.
    pub enter_hang: bool,
    /// How long a grip on a helicopter must hold, with the body hanging below it, before the hang
    /// is entered, seconds: a brief grab while climbing past does not.
    pub hang_hold_s: f32,
    /// How far below the gripped point the body's centre must be for the grip to count as a hang,
    /// metres.
    pub hang_below_m: f32,
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
            enter_hang: true,
            hang_hold_s: 0.3,
            hang_below_m: 0.3,
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
