//! The sim-rate experiment's setting.

use serde::{Deserialize, Serialize};

/// The sim-rate experiment's setting (`crate::sim_rate`). Only the debug UI reaches it: it is an
/// experiment for measuring cost and breakage, not a setting a player would change.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SimRateConfig {
    /// The tick rate the game runs at. The default, 30 Hz, is the game's own and leaves it
    /// untouched; the others are the experiment. Not saved, so every session starts at the game's
    /// own rate.
    #[serde(skip)]
    pub rate: SimRate,
}

impl SimRateConfig {
    pub const fn new() -> Self {
        Self {
            rate: SimRate::DEFAULT,
        }
    }
}

impl Default for SimRateConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A sim tick rate the experiment offers. The rates divide or match the common headset rates: 45
/// and 90 for a 90 Hz panel, 75 for a 75 Hz one, and 60 as the middle ground the render
/// interpolation covers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimRate {
    /// The game's own rate.
    #[serde(rename = "30")]
    Hz30,
    #[serde(rename = "45")]
    Hz45,
    #[serde(rename = "60")]
    Hz60,
    #[serde(rename = "75")]
    Hz75,
    #[serde(rename = "90")]
    Hz90,
}

impl SimRate {
    /// The game's own rate, at which the experiment changes nothing.
    pub const DEFAULT: SimRate = SimRate::Hz30;

    /// Every rate, slowest first.
    pub const ALL: [SimRate; 5] = [
        SimRate::Hz30,
        SimRate::Hz45,
        SimRate::Hz60,
        SimRate::Hz75,
        SimRate::Hz90,
    ];

    /// The rate in ticks per second.
    pub const fn hz(self) -> u32 {
        match self {
            SimRate::Hz30 => 30,
            SimRate::Hz45 => 45,
            SimRate::Hz60 => 60,
            SimRate::Hz75 => 75,
            SimRate::Hz90 => 90,
        }
    }
}
