//! The vehicle doors' settings.

use serde::{Deserialize, Serialize};

/// Opening vehicle doors by hand (see [`crate::controllers::physical_hands::doors`]): a grip on a
/// closed door's handle area takes hold of it, a pull unlatches it onto its hinge, and the hand
/// swings it from there. A shove swings it shut, and the game latches it.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoorsConfig {
    pub enabled: bool,
    /// How far outside a door's box a palm still reaches it, metres of the player's hand.
    pub reach_m: f32,
    /// How far from a door's handle a palm takes the door, whatever the reach's rays hit, metres of
    /// the player's hand.
    pub handle_reach_m: f32,
    /// Where on a closed door its handle area starts, as a share of the way from the door's hinge
    /// to its far edge, `0..=1`: a grip nearer the hinge than this takes nothing. Zero takes a
    /// closed door anywhere.
    pub handle_share: f32,
    /// How far a hand gripping a closed door must pull it outward before the latch gives, metres
    /// of the player's hand.
    pub unlatch_pull_m: f32,
    pub tearing: TearConfig,
}

impl DoorsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            reach_m: 0.06,
            handle_reach_m: 0.15,
            handle_share: 0.4,
            unlatch_pull_m: 0.05,
            tearing: TearConfig::new(),
        }
    }
}

/// Deliberate tearing after an open grip settles; distances and speeds use player metres.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TearConfig {
    pub enabled: bool,
    pub settle_s: f32,
    pub reset_lag_m: f32,
    pub yank_lag_m: f32,
    pub yank_speed_mps: f32,
    pub push_lag_m: f32,
    pub push_hold_s: f32,
    pub push_load: f32,
    pub limit_margin_rad: f32,
}

impl TearConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            settle_s: 0.2,
            reset_lag_m: 0.045,
            yank_lag_m: 0.18,
            yank_speed_mps: 1.8,
            push_lag_m: 0.14,
            push_hold_s: 0.18,
            push_load: 0.65,
            limit_margin_rad: 0.08,
        }
    }
}

impl Default for TearConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for DoorsConfig {
    fn default() -> Self {
        Self::new()
    }
}
