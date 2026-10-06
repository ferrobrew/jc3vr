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
    /// the stick sets and that changes only at the fixed rates below.
    pub enabled: bool,
    /// How fast the velocity grows toward the stick's, in metres per second squared.
    pub acceleration_mps2: f32,
    /// How fast the velocity shrinks when the stick asks for less of it or for another direction,
    /// in metres per second squared. Higher than the acceleration, so stopping is crisp.
    pub deceleration_mps2: f32,
    /// Where the target speed comes from.
    pub speed: SpeedSource,
    /// The full-stick speed under [`SpeedSource::Fixed`], in metres per second.
    pub run_speed_mps: f32,
}

impl StickConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            acceleration_mps2: 10.0,
            deceleration_mps2: 14.0,
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
    /// How long the filter may act without a break before the watchdog decides the animation
    /// state machine is waiting on an act the filter keeps from it, in seconds.
    pub watchdog_s: f32,
    /// How long the watchdog then lets every act through, in seconds.
    pub release_s: f32,
}

impl ActFilterConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            watchdog_s: 2.5,
            release_s: 2.0,
        }
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
    /// How long the anchor takes to hand over between the animated head and the root, seconds.
    pub blend_s: f32,
    /// The time constant of the smoothing of the root's height while grounded, seconds; zero turns
    /// it off. Turns a staircase's steps into a ramp.
    pub height_smoothing_s: f32,
    /// The furthest the smoothed height may trail the root's, metres.
    pub max_height_lag_m: f32,
    /// A height change beyond this in one tick is a teleport or a drop, and is taken at once.
    pub height_snap_m: f32,
    /// How long Rico must have stood still for his neutral head to be measured, seconds.
    pub capture_s: f32,
    /// How long after leaving the ground the anchor stays on the root, seconds, so a jump keeps the
    /// view steady while a long fall hands it back to the animation.
    pub jump_grace_s: f32,
}

impl StableViewConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            blend_s: 0.25,
            height_smoothing_s: 0.06,
            max_height_lag_m: 0.25,
            height_snap_m: 0.75,
            capture_s: 1.5,
            jump_grace_s: 1.0,
        }
    }
}

impl Default for StableViewConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Check the stick locomotion's settings. Paths are relative to `controllers.locomotion`.
pub(crate) fn validate(
    stick: &StickConfig,
    acts: &ActFilterConfig,
    view: &StableViewConfig,
) -> Result<(), Invalid> {
    positive("stick.acceleration_mps2", stick.acceleration_mps2, 1_000.0)?;
    positive("stick.deceleration_mps2", stick.deceleration_mps2, 1_000.0)?;
    positive("stick.run_speed_mps", stick.run_speed_mps, 50.0)?;
    positive("acts.watchdog_s", acts.watchdog_s, 60.0)?;
    nonnegative("acts.release_s", acts.release_s, 60.0)?;
    nonnegative("view.blend_s", view.blend_s, 10.0)?;
    nonnegative("view.height_smoothing_s", view.height_smoothing_s, 10.0)?;
    nonnegative("view.max_height_lag_m", view.max_height_lag_m, 10.0)?;
    positive("view.height_snap_m", view.height_snap_m, 100.0)?;
    positive("view.capture_s", view.capture_s, 60.0)?;
    nonnegative("view.jump_grace_s", view.jump_grace_s, 60.0)
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

fn nonnegative(name: &str, value: f32, max: f32) -> Result<(), Invalid> {
    if (0.0..=max).contains(&value) {
        Ok(())
    } else {
        Err(Invalid(format!(
            "locomotion.{name} is {value}, outside [0, {max}]"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_validate() {
        assert_eq!(
            validate(
                &StickConfig::new(),
                &ActFilterConfig::new(),
                &StableViewConfig::new()
            ),
            Ok(())
        );
    }

    #[test]
    fn a_non_finite_rate_is_rejected() {
        let stick = StickConfig {
            acceleration_mps2: f32::NAN,
            ..StickConfig::new()
        };
        assert!(validate(&stick, &ActFilterConfig::new(), &StableViewConfig::new()).is_err());
    }
}
