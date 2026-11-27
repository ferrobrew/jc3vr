//! The physics substrate's tuning, read from the shipped `tuning/physics.toml` ([`PHYSICS`]).

use std::f32::consts::PI;

use jc3vr_core::{
    config::engine::{Invalid, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The physics substrate's tuning file.
    pub static PHYSICS: PhysicsTuning = "physics.toml";
}

/// The physics substrate's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsTuning {
    pub rig: RigTuning,
    pub release: ReleaseTuning,
}

impl Tuning for PhysicsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range(
            "rig.unannounced_turn_rad",
            self.rig.unannounced_turn_rad,
            0.0,
            PI,
        )?;
        positive("release.max_hand_mps", self.release.max_hand_mps, 100.0)?;
        positive(
            "release.max_carried_mps",
            self.release.max_carried_mps,
            1_000.0,
        )?;
        range("release.max_age_s", self.release.max_age_s, 0.001, 1.0)
    }
}

/// The rig that carries the hands.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigTuning {
    /// The turn of the body's facing between two of a reader's ticks, with no event of the mod's,
    /// past which the game turned or placed the body rather than the body turning, radians
    /// ([`crate::controllers::physics::rig`]). Wider than any smooth turn makes in a tick.
    pub unannounced_turn_rad: f32,
}

/// What everything a hand lets go of leaves with ([`crate::controllers::physics::release`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseTuning {
    /// The fastest the hand's own share of a release leaves, metres per real second, against
    /// tracking glitches. The player's movement it carries is not counted.
    pub max_hand_mps: f32,
    /// The fastest movement of the player's a release carries, metres per second: past the game's
    /// fastest jets, so only a stale or corrupt reading is cut.
    pub max_carried_mps: f32,
    /// The oldest the rig's motion, the controller's publication, and the hand's latest sample may
    /// be for a release to use them, seconds.
    pub max_age_s: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(PHYSICS.get().validate(), Ok(()));
    }
}
