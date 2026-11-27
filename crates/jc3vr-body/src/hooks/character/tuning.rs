//! The character hooks' tuning, read from the shipped `tuning/character.toml` ([`CHARACTER`]): the
//! body IK ([`BodyIkTuning`]) and the neck's twist under the head override.

use jc3vr_core::{
    config::engine::{Invalid, positive},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::hooks::character::body_ik::tuning::BodyIkTuning;

jc3vr_core::tuning_file! {
    /// The character hooks' tuning file.
    pub static CHARACTER: CharacterTuning = "character.toml";
}

/// The character hooks' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterTuning {
    pub body_ik: BodyIkTuning,
    pub neck: NeckTuning,
}

impl Tuning for CharacterTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.body_ik.validate()?;
        positive("neck.twist_start_deg", self.neck.twist_start_deg, 180.0)?;
        positive("neck.twist_max_deg", self.neck.twist_max_deg, 180.0)
    }
}

/// How the neck bone twists along with the head under the head override.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeckTuning {
    /// The body-relative yaw, degrees, beyond which the neck bone twists along with the head. A
    /// real head only turns so far before the neck must follow, and with the hidden head bone
    /// carrying the whole yaw, leaving the neck animated knots the visible neck skinning once the
    /// head turns past it.
    pub(crate) twist_start_deg: f32,
    /// The most yaw the neck takes, degrees: the excess beyond
    /// [`twist_start_deg`](Self::twist_start_deg) goes to the neck up to this cap.
    pub(crate) twist_max_deg: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(CHARACTER.get().validate(), Ok(()));
    }
}
