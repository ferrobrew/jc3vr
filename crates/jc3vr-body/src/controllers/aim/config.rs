//! Per-hand aiming's settings.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::aim::sights::config::{SightTable, SightTuningConfig};

/// The per-hand aiming settings, at `controllers.aim` in the settings file; the payload binds the
/// slot at startup.
pub static AIM_CONFIG: ConfigSlot<AimConfig> = ConfigSlot::new("controllers.aim");

/// Per-hand aiming (see [`crate::controllers::aim`]). Acts only under the controller scheme. The
/// rays' fixed reach is [`RAY_LENGTH_M`](crate::controllers::aim::RAY_LENGTH_M).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AimConfig {
    /// Aim the held guns along the dominant hand's sight line (and, dual-wielding, each pistol
    /// along its own hand's).
    pub gun: bool,
    /// Aim the grapple along the off-hand controller's ray.
    pub grapple: bool,
    /// How strongly the written aim point is pulled toward the best candidate target along each
    /// hand ray, `0..=1`. Zero disables aim assist.
    pub assist_strength: f32,
    /// How far the grapple's aim turns toward the grapple target nearest its ray within
    /// [`grapple_assist_cone_deg`](Self::grapple_assist_cone_deg), `0..=1`: one aims at the
    /// target, zero disables the grapple's aim assist.
    pub grapple_assist_strength: f32,
    /// The half-angle of the cone around the grapple hand's ray that the grapple's aim assist
    /// looks for a target in, degrees.
    pub grapple_assist_cone_deg: f32,
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
            gun: true,
            grapple: true,
            assist_strength: 0.0,
            grapple_assist_strength: 1.0,
            grapple_assist_cone_deg: 5.0,
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
