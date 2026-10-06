//! The procedural body's settings, `controllers.body.procedural`.

use serde::{Deserialize, Serialize};

use crate::config::validate::Invalid;

/// The procedural body (see [`crate::controllers::body::procedural`]).
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProceduralBodyConfig {
    /// What the pose authority does at the HumanIK solve gate.
    pub mode: ProceduralMode,
    /// How long the body takes to hand over between the animation and the procedural pose, seconds.
    pub blend_s: f32,
    /// How far the hips move back for the head's lean forward, as a share of it: a lean becomes a
    /// hip hinge rather than a spine fold.
    pub counterbalance: f32,
    /// The lowest the hips go before the spine takes the rest of a crouch, as a share of their
    /// standing height.
    pub min_hips_height: f32,
    /// How far the hips turn toward the player's torso, at most, degrees.
    pub pelvis_turn_limit_deg: f32,
    /// How far the chest turns toward the player's torso, at most, degrees.
    pub chest_turn_limit_deg: f32,
    /// How far the spine leans from upright, at most, degrees.
    pub lean_limit_deg: f32,
    /// Zero the reach and pull of the game's own HumanIK targets on the bones the procedural body
    /// owns (the hips, waist, chest, shoulders, and legs), so the solve after the gate cannot move
    /// them.
    pub neutralize_targets: bool,
}

impl ProceduralBodyConfig {
    pub const fn new() -> Self {
        Self {
            // Off until the gate is confirmed in the headset: the pose written there has not yet
            // been seen to reach the solve and the hit shapes.
            mode: ProceduralMode::Off,
            blend_s: 0.3,
            counterbalance: 0.3,
            min_hips_height: 0.5,
            pelvis_turn_limit_deg: 35.0,
            chest_turn_limit_deg: 70.0,
            lean_limit_deg: 60.0,
            neutralize_targets: true,
        }
    }

    /// Check the settings. Paths are relative to `controllers.body.procedural`.
    pub(crate) fn validate(&self) -> Result<(), Invalid> {
        let checks = [
            ("blend_s", self.blend_s, 0.0, 10.0),
            ("counterbalance", self.counterbalance, 0.0, 1.0),
            ("min_hips_height", self.min_hips_height, 0.1, 1.0),
            (
                "pelvis_turn_limit_deg",
                self.pelvis_turn_limit_deg,
                0.0,
                180.0,
            ),
            (
                "chest_turn_limit_deg",
                self.chest_turn_limit_deg,
                0.0,
                180.0,
            ),
            ("lean_limit_deg", self.lean_limit_deg, 0.0, 90.0),
        ];
        for (name, value, min, max) in checks {
            if !(min..=max).contains(&value) {
                return Err(Invalid(format!(
                    "body.procedural.{name} is {value}, outside [{min}, {max}]"
                )));
            }
        }
        Ok(())
    }
}

impl Default for ProceduralBodyConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// What the pose authority does at the HumanIK solve gate.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProceduralMode {
    /// Nothing: the game's animation and the mod's IK targets pose the body as before.
    #[default]
    Off,
    /// Read the animated pose of the driven bones and write it back unchanged, and log how far the
    /// written pose reads back from what was written: proves the write path without moving
    /// anything.
    PassThrough,
    /// Build the standing body under the player's head while Rico stands on foot.
    Body,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_validate_and_an_out_of_range_value_does_not() {
        assert_eq!(ProceduralBodyConfig::new().validate(), Ok(()));
        let config = ProceduralBodyConfig {
            lean_limit_deg: f32::NAN,
            ..ProceduralBodyConfig::new()
        };
        assert!(config.validate().is_err());
    }
}
