//! The human hull's tuning, read from the shipped `tuning/human_hull.toml` ([`HUMAN_HULL`]): the
//! step-up, and the pacing and budgets of the native passes that discover and restore the hulls.

use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The human hull's tuning file.
    pub static HUMAN_HULL: HumanHullTuning = "human_hull.toml";
}

/// The human hull tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanHullTuning {
    pub step_up: StepUpTuning,
    pub restore: RestoreTuning,
    pub discovery: DiscoveryTuning,
}

impl Tuning for HumanHullTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.step_up.validate()?;
        self.restore.validate()?;
        count(
            "discovery.interval_passes",
            self.discovery.interval_passes.into(),
            1,
            100_000,
        )
    }
}

/// How the narrowed player is stepped up the steps the authored hull would climb (see
/// [`crate::controllers::physical_hands::human_hull::step_up`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepUpTuning {
    /// The slowest horizontal speed the controller drives the proxy at that counts as walking into
    /// something, metres per second.
    pub(crate) min_wanted_mps: f32,
    /// The share of the driven speed the player may still make along it and count as stopped,
    /// `0..=1`.
    pub(crate) blocked_share: f32,
    /// How far past the narrowed hull's edge the step's top is looked for, metres.
    pub(crate) probe_ahead_m: f32,
    /// How far above the authored cone's height the ray down starts, and the path ahead must be
    /// clear, metres.
    pub(crate) probe_above_m: f32,
    /// The least walkable top's normal height: the cosine of the steepest top stepped onto.
    pub(crate) min_top_normal_y: f32,
    /// How far above the step's top the feet are lifted, metres.
    pub(crate) lift_margin_m: f32,
    /// The time a lift takes to raise the feet to the step's top, seconds.
    pub(crate) lift_time_s: f32,
    /// The fastest lift, metres per second.
    pub(crate) max_lift_mps: f32,
    /// The longest a lift lasts, seconds: a step the proxy does not clear by then is let go.
    pub(crate) lift_hold_s: f32,
}

impl StepUpTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("step_up.min_wanted_mps", self.min_wanted_mps, 20.0)?;
        range("step_up.blocked_share", self.blocked_share, 0.0, 1.0)?;
        nonnegative("step_up.probe_ahead_m", self.probe_ahead_m, 2.0)?;
        nonnegative("step_up.probe_above_m", self.probe_above_m, 2.0)?;
        range("step_up.min_top_normal_y", self.min_top_normal_y, 0.0, 1.0)?;
        nonnegative("step_up.lift_margin_m", self.lift_margin_m, 1.0)?;
        positive("step_up.lift_time_s", self.lift_time_s, 5.0)?;
        positive("step_up.max_lift_mps", self.max_lift_mps, 20.0)?;
        nonnegative("step_up.lift_hold_s", self.lift_hold_s, 10.0)
    }
}

/// How the native passes pace and bound the restoration of the authored hulls.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreTuning {
    /// How many shutdown attempts wait for the native phase to restore every narrowed body before
    /// the eject leaves the hulls in place. The lifecycle retries every few frames: each body
    /// restores once its own surroundings are clear, which a body pressed against a wall may never
    /// be, and an idle subsystem must not hold the eject indefinitely.
    pub(crate) shutdown_attempts: u32,
    /// The longest wait between attempts while disabled and some body stays obstructed, in
    /// pre-simulation passes.
    pub(crate) max_retry_wait_passes: u32,
    /// The candidates one pass may examine across all of its bodies.
    pub(crate) max_candidates: usize,
    /// The shape leaves one pass's clearance queries may examine across all of its bodies.
    pub(crate) max_leaf_work: usize,
}

impl RestoreTuning {
    fn validate(&self) -> Result<(), Invalid> {
        count(
            "restore.shutdown_attempts",
            self.shutdown_attempts.into(),
            1,
            1000,
        )?;
        count(
            "restore.max_retry_wait_passes",
            self.max_retry_wait_passes.into(),
            1,
            100_000,
        )?;
        count(
            "restore.max_candidates",
            self.max_candidates as u64,
            1,
            100_000_000,
        )?;
        count(
            "restore.max_leaf_work",
            self.max_leaf_work as u64,
            1,
            1_000_000_000,
        )
    }
}

/// How often the native pass walks the live bodies for settings tables it has not adopted.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryTuning {
    /// The longest a table whose characters stream in waits to be found, in pre-simulation passes.
    pub(crate) interval_passes: u32,
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
        assert_eq!(HUMAN_HULL.get().validate(), Ok(()));
    }
}
