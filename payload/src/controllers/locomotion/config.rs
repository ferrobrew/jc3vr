//! The stick locomotion's settings: the velocity integrator ([`StickConfig`]), the act filter
//! ([`ActFilterConfig`]), and the stable view anchor ([`StableViewConfig`]). They live under
//! `controllers.locomotion` beside the stick's direction frame.

use serde::{Deserialize, Serialize};

use crate::config::validate::Invalid;

/// Stick-driven on-foot velocity with fixed acceleration (see [`crate::controllers::locomotion`]).
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StickConfig {
    /// Replace the animation's root velocity with the stick's, so Rico moves at a speed that only
    /// the stick sets and that changes only at fixed rates
    /// ([`Rates::FIXED`](crate::controllers::locomotion::accel::Rates::FIXED)).
    pub enabled: bool,
    /// Where the target speed comes from.
    pub speed: SpeedSource,
    /// The full-stick speed under [`SpeedSource::Fixed`], in metres per second.
    pub run_speed_mps: f32,
}

impl StickConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            speed: SpeedSource::Game,
            run_speed_mps: 4.5,
        }
    }
}

impl Default for StickConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Where the stick locomotion's target speed comes from.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SpeedSource {
    /// The game's own target speed for the stick: its walk, run, and backpedal speeds, and zero
    /// where it would stop Rico at a ledge or a wall. Keeps distances and chases as balanced.
    #[default]
    Game,
    /// The stick's magnitude times [`StickConfig::run_speed_mps`].
    Fixed,
}

/// The filter that keeps the turning and turning-start animations off the local player while the
/// stick drives the body (see [`crate::controllers::locomotion::acts`]).
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActFilterConfig {
    /// Drop the turn-in-place acts and swap the turning starts and stops for their straight
    /// variants.
    pub enabled: bool,
}

impl ActFilterConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for ActFilterConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The on-foot camera anchor fixed to Rico's root (see [`crate::controllers::locomotion::view`]).
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StableViewConfig {
    /// Anchor the view to Rico's root plus his neutral standing head on foot, instead of to the
    /// animated head, so no animation bob, lean, or lurch reaches the view.
    pub enabled: bool,
}

impl StableViewConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for StableViewConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Check the stick locomotion's settings. Paths are relative to `controllers.locomotion`.
pub(crate) fn validate(stick: &StickConfig) -> Result<(), Invalid> {
    positive("stick.run_speed_mps", stick.run_speed_mps, 50.0)
}

fn positive(name: &str, value: f32, max: f32) -> Result<(), Invalid> {
    if value > 0.0 && value <= max {
        Ok(())
    } else {
        Err(Invalid(format!(
            "locomotion.{name} is {value}, outside (0, {max}]"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_validate() {
        assert_eq!(validate(&StickConfig::new()), Ok(()));
    }

    #[test]
    fn a_non_finite_speed_is_rejected() {
        let stick = StickConfig {
            run_speed_mps: f32::NAN,
            ..StickConfig::new()
        };
        assert!(validate(&stick).is_err());
    }
}
