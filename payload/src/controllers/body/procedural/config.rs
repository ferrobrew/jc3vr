//! The procedural body's settings, `controllers.body.procedural`.

use serde::{Deserialize, Serialize};

use crate::{config::validate::Invalid, controllers::body::procedural::stance::StanceConfig};

/// The procedural body (see [`crate::controllers::body::procedural`]).
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProceduralBodyConfig {
    /// What the pose authority does at the HumanIK solve gate.
    pub mode: ProceduralMode,
    /// How long a region of the body takes to hand over from the animation to the procedural pose,
    /// seconds: the smoothing time of its spring.
    pub blend_s: f32,
    /// How long a region takes to hand back to the animation, seconds; shorter, since the game's
    /// authored motion (a vehicle entry, a climb, a fall) is under way when it does.
    pub blend_out_s: f32,
    /// How far the chest turns toward the player's torso, at most, degrees.
    pub chest_turn_limit_deg: f32,
    /// How far the spine leans from upright, at most, degrees.
    pub lean_limit_deg: f32,
    /// A point of the chest higher than this below the camera's eyes is kept out of the near
    /// field, metres.
    pub chest_clear_below_m: f32,
    /// How far behind the eyes, along the view's level forward, the chest is kept, metres.
    pub chest_clear_back_m: f32,
    /// Zero the reach and pull of the game's own HumanIK targets on the bones the procedural body
    /// owns (the hips, waist, chest, shoulders, and legs), so the solve after the gate cannot move
    /// them.
    pub neutralize_targets: bool,
    /// The chest capsule's radius, metres.
    pub chest_radius_m: f32,
    /// The abdomen capsule's radius, metres.
    pub abdomen_radius_m: f32,
    /// How far outside the torso a pushed wrist or elbow lands, metres: the hand's half-thickness.
    pub hand_margin_m: f32,
    /// Hint where each driven arm's elbow goes, with a weak target on the elbow.
    pub elbow_hints: bool,
    /// The elbow hint's reach: how strongly the solve pulls the elbow toward the hint.
    pub elbow_hint_reach: f32,
    /// How far a clavicle lifts for a hand overhead, at most, degrees.
    pub clavicle_elevation_deg: f32,
    /// How far a clavicle drops for a hand reaching far down, at most, degrees.
    pub clavicle_depression_deg: f32,
    /// How far a clavicle swings forward for a hand reaching far ahead or across, at most,
    /// degrees.
    pub clavicle_protraction_deg: f32,
    /// The share of the arm's length at which a reach ahead or down starts to take the clavicle;
    /// it takes it fully at the arm's full length.
    pub clavicle_reach_start: f32,
    /// How quickly the clavicles follow the hands, seconds: the time constant of their follower.
    pub clavicle_follow_s: f32,
    /// How long a hit reaction (a stumble or a knock-back) holds the body on the animation after
    /// its act, seconds: the game keeps no state for the reaction's length.
    pub hit_reaction_hold_s: f32,
    /// Rico's feet squared under him while he stands idle on foot.
    pub stance: StanceConfig,
}

impl ProceduralBodyConfig {
    pub const fn new() -> Self {
        Self {
            mode: ProceduralMode::Full,
            blend_s: 0.35,
            blend_out_s: 0.2,
            chest_turn_limit_deg: 70.0,
            lean_limit_deg: 60.0,
            chest_clear_below_m: 0.25,
            chest_clear_back_m: 0.08,
            neutralize_targets: true,
            chest_radius_m: 0.14,
            abdomen_radius_m: 0.13,
            hand_margin_m: 0.03,
            elbow_hints: true,
            elbow_hint_reach: 0.3,
            clavicle_elevation_deg: 30.0,
            clavicle_depression_deg: 10.0,
            clavicle_protraction_deg: 25.0,
            clavicle_reach_start: 0.8,
            clavicle_follow_s: 0.1,
            hit_reaction_hold_s: 1.2,
            stance: StanceConfig::new(),
        }
    }

    /// Check the settings. Paths are relative to `controllers.body.procedural`.
    pub(crate) fn validate(&self) -> Result<(), Invalid> {
        let checks = [
            ("blend_s", self.blend_s, 0.0, 10.0),
            ("blend_out_s", self.blend_out_s, 0.0, 10.0),
            (
                "chest_turn_limit_deg",
                self.chest_turn_limit_deg,
                0.0,
                180.0,
            ),
            ("lean_limit_deg", self.lean_limit_deg, 0.0, 90.0),
            ("chest_clear_below_m", self.chest_clear_below_m, 0.0, 1.0),
            ("chest_clear_back_m", self.chest_clear_back_m, 0.0, 0.5),
            ("chest_radius_m", self.chest_radius_m, 0.0, 0.4),
            ("abdomen_radius_m", self.abdomen_radius_m, 0.0, 0.4),
            ("hand_margin_m", self.hand_margin_m, 0.0, 0.2),
            ("elbow_hint_reach", self.elbow_hint_reach, 0.0, 1.0),
            (
                "clavicle_elevation_deg",
                self.clavicle_elevation_deg,
                0.0,
                45.0,
            ),
            (
                "clavicle_depression_deg",
                self.clavicle_depression_deg,
                0.0,
                30.0,
            ),
            (
                "clavicle_protraction_deg",
                self.clavicle_protraction_deg,
                0.0,
                45.0,
            ),
            ("clavicle_reach_start", self.clavicle_reach_start, 0.3, 0.99),
            ("clavicle_follow_s", self.clavicle_follow_s, 0.0, 1.0),
            ("hit_reaction_hold_s", self.hit_reaction_hold_s, 0.0, 5.0),
            ("stance.width_m", self.stance.width_m, 0.0, 0.8),
            ("stance.toe_out_deg", self.stance.toe_out_deg, -30.0, 60.0),
            ("stance.blend_s", self.stance.blend_s, 0.0, 5.0),
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
    /// Pose the torso from the player's head and hands on the animation's hips, wherever the
    /// hand-over table gives the torso to the player. Saved files from before name it `Body`.
    #[serde(alias = "Body")]
    Full,
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
