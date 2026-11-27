//! The physics substrate's tuning, read from the shipped `tuning/physics.toml` ([`PHYSICS`]).

use std::f32::consts::PI;

use jc3vr_core::{
    config::engine::{Invalid, range},
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
}

impl Tuning for PhysicsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range(
            "rig.unannounced_turn_rad",
            self.rig.unannounced_turn_rad,
            0.0,
            PI,
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(PHYSICS.get().validate(), Ok(()));
    }
}
