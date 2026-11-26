//! The stick locomotion's settings: the stick's direction frame and its velocity integrator's
//! switch ([`StickConfig`]), under `controllers.locomotion`.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

/// Stick-driven on-foot velocity with fixed acceleration (see [`crate::controllers::locomotion`]).
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StickConfig {
    /// Replace the animation's root velocity with the stick's, so Rico moves at the game's target
    /// speed for the stick and that speed changes only at fixed rates
    /// (the locomotion tuning's `stick.rates`).
    pub enabled: bool,
}

impl StickConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for StickConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The stick locomotion settings, at `controllers.locomotion` in the settings file; the payload
/// binds the slot at startup.
pub static LOCOMOTION_CONFIG: ConfigSlot<LocomotionConfig> =
    ConfigSlot::new("controllers.locomotion");

/// On-foot stick locomotion. Turning is configured in `headpose.vr_turn` (see the module docs).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LocomotionConfig {
    /// The frame the off-hand stick's direction is relative to.
    pub direction: LocomotionDirection,
    /// The stick's velocity, with fixed acceleration.
    pub stick: StickConfig,
}

impl LocomotionConfig {
    pub const fn new() -> Self {
        Self {
            direction: LocomotionDirection::Head,
            stick: StickConfig::new(),
        }
    }
}

impl Default for LocomotionConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The frame the locomotion stick's direction is relative to.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LocomotionDirection {
    /// Forward is where the head faces (yaw only).
    #[default]
    Head,
    /// Forward is where the off-hand controller points (yaw only).
    OffHand,
}
