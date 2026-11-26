//! Per-hand aiming's settings.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::aim::sights::config::{SightTable, SightTuningConfig};

/// The per-hand aiming settings, at `controllers.aim` in the settings file; the payload binds the
/// slot at startup.
pub static AIM_CONFIG: ConfigSlot<AimConfig> = ConfigSlot::new("controllers.aim");

/// Per-hand aiming (see [`crate::controllers::aim`]). Acts only under the controller scheme, where
/// the guns aim along their hands' sight lines and the grapple along the off-hand's ray. The
/// rays' fixed reach is [`ray_length_m`](crate::controllers::aim::ray_length_m).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AimConfig {
    /// How strongly the written aim point is pulled toward the best candidate target along each
    /// hand ray, `0..=1`. Zero disables aim assist.
    pub assist_strength: f32,
    /// Per-weapon corrections to the sight line derived from the fire point. An entry here takes
    /// precedence over the embedded sight table
    /// ([`crate::controllers::aim::sights`]); a weapon in neither aims from the fire point.
    pub sights: SightTable,
    /// The in-headset sight tuning tool.
    pub sight_tuning: SightTuningConfig,
}

impl AimConfig {
    pub const fn new() -> Self {
        Self {
            assist_strength: 0.0,
            sights: SightTable::EMPTY,
            sight_tuning: SightTuningConfig::new(),
        }
    }
}

impl Default for AimConfig {
    fn default() -> Self {
        Self::new()
    }
}
