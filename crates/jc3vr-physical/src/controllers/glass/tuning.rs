//! Breaking glass's tuning, read from the shipped `tuning/glass.toml` ([`GLASS`]): how strikes on
//! glass are measured and what they do, and how long a pane's strikes are remembered.

use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// Breaking glass's tuning file.
    pub static GLASS: GlassTuning = "glass.toml";
}

/// Breaking glass's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlassTuning {
    pub strike: StrikeTuning,
    pub panes: PanesTuning,
}

impl Tuning for GlassTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.strike.validate()?;
        self.panes.validate()
    }
}

/// How strikes on glass are measured and what they do (see [`crate::controllers::glass::strike`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrikeTuning {
    /// The slowest strike that damages glass, world metres per second: a hand resting against a
    /// window, or pushed slowly through it, does nothing.
    pub min_speed_mps: f32,
    /// The speed at and past which a strike does [`max_damage`](Self::max_damage).
    pub full_speed_mps: f32,
    /// The damage of the slowest strike that counts, in the game's damage-point strength (the
    /// strength a bullet adds is on `MetaWindow::AddWindowDamagePoint`).
    pub(crate) min_damage: f32,
    /// The damage of a strike at [`full_speed_mps`](Self::full_speed_mps).
    pub(crate) max_damage: f32,
    /// One strike at this speed or faster brings the whole pane down.
    pub(crate) shatter_speed_mps: f32,
    /// A pane whose damage reaches this brings itself down: the total of its damage points, each
    /// counted up to one, plus what blows on a point already at full strength added past it.
    pub(crate) shatter_total: f32,
    /// How long a striker waits before it strikes again, seconds, so that one swing strikes once.
    pub cooldown_s: f32,
    /// How far past the palm its sweep reaches, metres.
    pub(crate) palm_lead_m: f32,
    /// How far past the muzzle a gun's sweep reaches, metres.
    pub(crate) tip_lead_m: f32,
    /// How far ahead of the hand's aim pose a gun without measured geometry (a pistol) has its
    /// muzzle, metres.
    pub(crate) pistol_reach_m: f32,
    /// The longest time between two samples of a striker that still makes a swing, seconds.
    pub(crate) max_gap_s: f32,
}

/// The panes whose strikes are remembered, for the damage their point lists could not take.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanesTuning {
    /// How long a pane's strikes are remembered after its last, seconds.
    pub(crate) memory_s: f32,
    /// The most panes whose strikes are remembered at once; the oldest is forgotten past it.
    pub max_records: usize,
}

/// The fastest any strike speed may be set to, world metres per second.
const MAX_SPEED_MPS: f32 = 1000.0;

impl StrikeTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("strike.min_speed_mps", self.min_speed_mps, MAX_SPEED_MPS)?;
        range(
            "strike.full_speed_mps",
            self.full_speed_mps,
            self.min_speed_mps,
            MAX_SPEED_MPS,
        )?;
        nonnegative("strike.min_damage", self.min_damage, 100.0)?;
        positive("strike.max_damage", self.max_damage, 100.0)?;
        positive(
            "strike.shatter_speed_mps",
            self.shatter_speed_mps,
            MAX_SPEED_MPS,
        )?;
        positive("strike.shatter_total", self.shatter_total, 1000.0)?;
        nonnegative("strike.cooldown_s", self.cooldown_s, 60.0)?;
        nonnegative("strike.palm_lead_m", self.palm_lead_m, 10.0)?;
        nonnegative("strike.tip_lead_m", self.tip_lead_m, 10.0)?;
        nonnegative("strike.pistol_reach_m", self.pistol_reach_m, 10.0)?;
        positive("strike.max_gap_s", self.max_gap_s, 60.0)
    }
}

impl PanesTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("panes.memory_s", self.memory_s, 86_400.0)?;
        if !(1..=4096).contains(&self.max_records) {
            return Err(Invalid(format!(
                "panes.max_records must be in [1, 4096], got {}",
                self.max_records
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(GLASS.get().validate(), Ok(()));
    }
}
