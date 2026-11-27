//! The hand climbing's and the push-up's tuning, read from the shipped `tuning/climb.toml`
//! ([`CLIMB`]).

use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    published::{StepWindow, Window},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The hand climbing's and the push-up's tuning file.
    pub static CLIMB: ClimbTuning = "climb.toml";
}

/// The climbing tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimbTuning {
    pub climb: HandClimbTuning,
    pub vault: VaultTuning,
}

impl Tuning for ClimbTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.climb.validate()?;
        self.vault.validate()
    }
}

/// The hand climbing's tuning (see [`crate::controllers::physical_hands::climb`]): an empty hand
/// whose body touches the world grips it with the grip, the player's body moves against the hands'
/// motion, and the body is carried over a ledge the hands grip the top of once they have pulled it
/// near the top.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandClimbTuning {
    /// How far past the palm a surface may be for the grip to take it, metres of the player's hand.
    pub reach_m: f32,
    /// The share of the gap between where the hands would have the body and where it is that one
    /// physics step closes, `0..=1`.
    pub gain: f32,
    /// The fastest the hands move the body, metres per second.
    pub max_speed_mps: f32,
    /// How far the body may be from where the hands would have it before the grips let go, metres:
    /// a hand pulled much further than the body can follow, through a wall or out of reach.
    pub(crate) break_distance_m: f32,
    /// The least level share of a gripped surface's normal for it to count as a face the body rises
    /// along, rather than a ledge's top ([`slope_follow`]).
    ///
    /// [`slope_follow`]: crate::controllers::physical_hands::climb::solve::slope_follow
    pub(crate) min_face_level: f32,
    /// The share of the climbing speed the body keeps when the last grip lets go, so that a pull
    /// flings the player.
    pub(crate) fling_scale: f32,
    /// The fastest fling, metres per second.
    pub(crate) max_fling_speed_mps: f32,
    /// How long the climbing speed a fling keeps is smoothed over, seconds.
    pub(crate) fling_smoothing_s: f32,
    /// How long a fling keeps carrying the body after the release, seconds, as a throw under
    /// gravity, until the player stands on something or grips again.
    pub(crate) fling_carry_s: f32,
    /// How far below the gripped top the feet may be for the mantle to start, metres.
    pub(crate) mantle_start_m: f32,
    /// How far above the gripped top the mantle lifts the feet before it carries the body over,
    /// metres.
    pub(crate) mantle_clearance_m: f32,
    /// The share of [`HandClimbTuning::mantle_clearance_m`] the feet may still be below where the
    /// mantle carries them and go over the top rather than straight up, `0..=1`.
    pub(crate) mantle_over_below_clearance_share: f32,
    /// How far past the grip the mantle carries the body, metres.
    pub(crate) mantle_forward_m: f32,
    /// The mantle's speed, metres per second.
    pub(crate) mantle_speed_mps: f32,
    /// The least smoothed upward climbing speed for the mantle to start, metres per second, so that
    /// a twitch upward near a ledge does not start one.
    pub(crate) mantle_min_speed_mps: f32,
    /// How far the feet must have climbed since the climb started for the mantle to start while
    /// the player still stands on something, metres.
    pub(crate) mantle_min_climb_m: f32,
    /// How far past the ledge's top the mantle carries the feet before it lets them down onto it,
    /// metres: about the player capsule's radius.
    pub(crate) mantle_land_past_m: f32,
    /// How fast the mantle lets the feet down onto the ledge once they are over it, metres per
    /// second.
    pub(crate) mantle_land_speed_mps: f32,
    /// How close to where the mantle carries them the feet count as there, metres.
    pub(crate) mantle_arrived_m: f32,
    /// How far below the gripped top feet that stand count as on the ledge, metres.
    pub(crate) mantle_landed_below_m: f32,
    /// How many steps a mantle may take before it gives up, counted at the game's simulation tick.
    pub(crate) max_mantle_steps: u32,
    /// The least upward share of a gripped surface's normal for it to count as a ledge's top,
    /// `0..=1`.
    pub(crate) min_ledge_normal_y: f32,
    /// The climbing speed of the strongest tension pulse, metres per second.
    pub(crate) tension_full_speed_mps: f32,
    /// The least share of the tension pulse that pulses a gripping hand, `0..=1`.
    pub(crate) min_tension: f32,
    /// The lightest dynamic body a hand climbs, kilograms; a lighter one is a loose object.
    pub(crate) min_moving_mass_kg: f32,
    /// The fastest a gripped body's motion carries the climber, metres per second.
    pub(crate) max_carry_speed_mps: f32,
    /// How long a publish stays current for the input tick, which runs every frame, milliseconds.
    pub stale_after_ms: u64,
    /// How many steps in a row may run without a publish, or with a gripping hand's body not
    /// acting, before the grips let go: the aim update runs every simulation tick, and a game
    /// pause stops the steps as well as the publishes.
    pub(crate) max_stale_steps: u32,
}

impl HandClimbTuning {
    /// [`HandClimbTuning::stale_after_ms`] as a freshness window.
    pub fn stale_after(&self) -> Window {
        Window::from_millis(self.stale_after_ms)
    }

    /// How many physics steps the palms' tick's last run stays current for a step: the step after
    /// it takes it, and [`HandClimbTuning::max_stale_steps`] more may run without another.
    pub(crate) fn publish_window(&self) -> StepWindow {
        StepWindow::new(u64::from(self.max_stale_steps) + 1)
    }

    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("climb.reach_m", self.reach_m, 1.0)?;
        range("climb.gain", self.gain, 0.0, 1.0)?;
        positive("climb.max_speed_mps", self.max_speed_mps, 100.0)?;
        positive("climb.break_distance_m", self.break_distance_m, 100.0)?;
        range("climb.min_face_level", self.min_face_level, 0.0, 1.0)?;
        nonnegative("climb.fling_scale", self.fling_scale, 10.0)?;
        nonnegative("climb.max_fling_speed_mps", self.max_fling_speed_mps, 100.0)?;
        nonnegative("climb.fling_smoothing_s", self.fling_smoothing_s, 10.0)?;
        nonnegative("climb.fling_carry_s", self.fling_carry_s, 10.0)?;
        nonnegative("climb.mantle_start_m", self.mantle_start_m, 10.0)?;
        nonnegative("climb.mantle_clearance_m", self.mantle_clearance_m, 2.0)?;
        range(
            "climb.mantle_over_below_clearance_share",
            self.mantle_over_below_clearance_share,
            0.0,
            1.0,
        )?;
        nonnegative("climb.mantle_forward_m", self.mantle_forward_m, 5.0)?;
        positive("climb.mantle_speed_mps", self.mantle_speed_mps, 20.0)?;
        nonnegative(
            "climb.mantle_min_speed_mps",
            self.mantle_min_speed_mps,
            20.0,
        )?;
        nonnegative("climb.mantle_min_climb_m", self.mantle_min_climb_m, 10.0)?;
        nonnegative("climb.mantle_land_past_m", self.mantle_land_past_m, 5.0)?;
        nonnegative(
            "climb.mantle_land_speed_mps",
            self.mantle_land_speed_mps,
            20.0,
        )?;
        positive("climb.mantle_arrived_m", self.mantle_arrived_m, 1.0)?;
        nonnegative(
            "climb.mantle_landed_below_m",
            self.mantle_landed_below_m,
            2.0,
        )?;
        count(
            "climb.max_mantle_steps",
            self.max_mantle_steps.into(),
            1,
            10_000,
        )?;
        range(
            "climb.min_ledge_normal_y",
            self.min_ledge_normal_y,
            0.0,
            1.0,
        )?;
        positive(
            "climb.tension_full_speed_mps",
            self.tension_full_speed_mps,
            100.0,
        )?;
        range("climb.min_tension", self.min_tension, 0.0, 1.0)?;
        nonnegative("climb.min_moving_mass_kg", self.min_moving_mass_kg, 1.0e6)?;
        nonnegative(
            "climb.max_carry_speed_mps",
            self.max_carry_speed_mps,
            1000.0,
        )?;
        count("climb.stale_after_ms", self.stale_after_ms, 1, 60_000)?;
        count(
            "climb.max_stale_steps",
            self.max_stale_steps.into(),
            0,
            1000,
        )
    }
}

/// The push-up's tuning (see [`crate::controllers::physical_hands::vault`]): a hand body pressed
/// down on a surface lifts the player, a jump assist bounded by how far the hand presses and how
/// high the player has risen.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultTuning {
    /// The downward press left alone before it lifts, metres of the player's hand, so that a hand
    /// resting on a table lifts nothing.
    pub deadband_m: f32,
    /// The lifting speed per metre of press past the deadband, per second.
    pub stiffness_per_s: f32,
    /// The fastest lift, metres per second.
    pub(crate) max_lift_mps: f32,
    /// The least downward share of a press for it to lift, `0..=1`.
    pub(crate) min_vertical: f32,
    /// The highest the feet rise above where the push started, metres.
    pub(crate) max_rise_m: f32,
    /// How far above the feet a surface must be for a press on it to lift, metres of the game
    /// world: the ground the player stands on, and anything barely above it (a kerb, a step, the
    /// floor where the player's real floor sits a little lower than the game's), is never pushed
    /// on; a hand reaching down for something on the ground rests on it instead.
    pub(crate) min_height_m: f32,
    /// How long a press must stay past the deadband before it starts to lift, seconds: a hand
    /// that brushes a surface on its way past, or lands on it with a moment's overshoot, lifts
    /// nothing, and a deliberate push lifts.
    pub hold_s: f32,
    /// The fastest the player may be moving down for the hands to lift them, metres per second:
    /// a player falling faster is never lifted, so that a fall is not stopped. Above zero, so that
    /// a player settling onto the ground or stepping down a slope still pushes up.
    pub(crate) fall_tolerance_mps: f32,
}

impl VaultTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("vault.deadband_m", self.deadband_m, 1.0)?;
        nonnegative("vault.stiffness_per_s", self.stiffness_per_s, 1000.0)?;
        nonnegative("vault.max_lift_mps", self.max_lift_mps, 20.0)?;
        range("vault.min_vertical", self.min_vertical, 0.0, 1.0)?;
        nonnegative("vault.max_rise_m", self.max_rise_m, 10.0)?;
        nonnegative("vault.min_height_m", self.min_height_m, 5.0)?;
        nonnegative("vault.hold_s", self.hold_s, 5.0)?;
        nonnegative("vault.fall_tolerance_mps", self.fall_tolerance_mps, 20.0)
    }
}

/// Check that the count `value` is in `[min, max]`.
fn count(name: &str, value: u64, min: u64, max: u64) -> Result<(), Invalid> {
    if !(min..=max).contains(&value) {
        return Err(Invalid(format!(
            "{name} must be in [{min}, {max}], got {value}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(CLIMB.get().validate(), Ok(()));
    }
}
