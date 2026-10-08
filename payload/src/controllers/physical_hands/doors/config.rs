//! The vehicle doors' settings.

use serde::{Deserialize, Serialize};

use crate::controllers::weapons::config::PulseSpec;

/// Opening vehicle doors by hand (see [`crate::controllers::physical_hands::doors`]): a grip on a
/// closed door's handle area takes hold of it, a pull unlatches it onto its hinge, and the hand
/// swings it from there. A shove swings it shut, and the game latches it.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoorsConfig {
    pub enabled: bool,
    /// How far outside a door's box a palm still reaches it, metres of the player's hand.
    pub reach_m: f32,
    /// Where on a closed door its handle area starts, as a share of the way from the door's hinge
    /// to its far edge, `0..=1`: a grip nearer the hinge than this takes nothing. Zero takes a
    /// closed door anywhere.
    pub handle_share: f32,
    /// How far a hand gripping a closed door must pull it outward before the latch gives, metres
    /// of the player's hand.
    pub unlatch_pull_m: f32,
    /// The pulse of the latch giving.
    pub unlatch_pulse: PulseSpec,
}

impl DoorsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            reach_m: 0.06,
            handle_share: 0.4,
            unlatch_pull_m: 0.05,
            unlatch_pulse: PulseSpec {
                amplitude: 0.7,
                duration_s: 0.05,
            },
        }
    }
}

impl Default for DoorsConfig {
    fn default() -> Self {
        Self::new()
    }
}
