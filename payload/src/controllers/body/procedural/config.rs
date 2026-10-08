//! The procedural body's settings, `controllers.body.procedural`.

use serde::{Deserialize, Serialize};

use crate::config::validate::Invalid;

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
    /// How far the hips move back for the head's lean forward, as a share of it: a lean becomes a
    /// hip hinge rather than a spine fold.
    pub counterbalance: f32,
    /// How quickly the hips follow their place over the stance, seconds: the time constant of
    /// their horizontal follower, so a quick lean bends the spine before the hips go with it.
    pub hips_follow_s: f32,
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
    /// Hold the feet in the world, stepping when the body moves or turns away from them, instead
    /// of leaving them where the animation puts them.
    pub plant_feet: bool,
    /// How far a planted foot may be from where the animation would put it before it steps,
    /// metres.
    pub step_threshold_m: f32,
    /// How far a planted foot may be turned from the animation's before it steps, degrees.
    pub step_turn_deg: f32,
    /// How long a step takes, seconds.
    pub step_duration_s: f32,
    /// How high a step lifts the foot, metres.
    pub step_height_m: f32,
    /// Walk the feet while the body moves (phase-driven steps matched to the body's velocity),
    /// instead of leaving the moving body to the animation.
    pub gait: bool,
    /// The speed the feet start walking at, metres per second; below it they step as standing.
    pub gait_speed_mps: f32,
    /// The walking speed, metres per second, at which the walk's tuning is the `walk_*` values.
    pub walk_speed_mps: f32,
    /// The distance travelled in one walking cycle (a step of each foot), metres.
    pub walk_stride_m: f32,
    /// The share of a walking cycle each foot spends on the ground.
    pub walk_duty: f32,
    /// How high a walking step lifts the foot, metres.
    pub walk_step_height_m: f32,
    /// The running speed, metres per second, at which the walk's tuning is the `run_*` values.
    pub run_speed_mps: f32,
    /// The distance travelled in one running cycle, metres.
    pub run_stride_m: f32,
    /// The share of a running cycle each foot spends on the ground; below a half, both feet leave
    /// it for a moment.
    pub run_duty: f32,
    /// How high a running step lifts the foot, metres.
    pub run_step_height_m: f32,
    /// Shorten and lower the steps in a crouch by how far the hips have dropped, so a crouched
    /// walk takes short, quick, low steps instead of the upright walk's strides.
    pub crouch_gait: bool,
    /// The stride at the hips' lowest, as a share of the upright walk's.
    pub crouch_stride_share: f32,
    /// The lift of a step at the hips' lowest, as a share of the upright walk's.
    pub crouch_lift_share: f32,
    /// The least share of a cycle each foot spends on the ground at the hips' lowest.
    pub crouch_duty: f32,
    /// Kneel on one knee when the head goes below the floor of a squat and stays there, still and
    /// not moving: the trailing foot's knee down, or the off hand's side.
    pub kneel: bool,
    /// How long the kneel takes to ease in or out, seconds.
    pub kneel_blend_s: f32,
    /// Find the ground under each foothold with a scene ray, so the feet stand on slopes and
    /// stairs, instead of taking the root's height.
    pub ground_casts: bool,
    /// How far above the root a foothold's ground is looked for, metres: the highest step up.
    pub max_step_up_m: f32,
    /// How far below the root a foothold's ground is looked for, metres: the lowest step down.
    pub max_step_down_m: f32,
    /// How far a foot tilts to the slope it stands on, at most, degrees.
    pub ankle_tilt_limit_deg: f32,
    /// How quickly the hips drop for a foot the legs cannot otherwise reach, seconds: the time
    /// constant of the drop's follower (it rises back at a third of the pace).
    pub reach_drop_s: f32,
    /// Keep the wrist and elbow targets out of the procedural torso (a chest and an abdomen
    /// capsule along the spine), so the arms do not pass through the body.
    pub keep_out: bool,
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
    /// Turn each clavicle toward its hand when the hand is raised above the shoulder or reaches
    /// toward the end of the arm, so the shoulder goes with an overhead or far reach.
    pub clavicles: bool,
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
    /// Pose the legs in a jump or a short fall: tucked under the hips while rising and let down
    /// while falling, instead of the animation's legs under the procedural hips.
    pub air_legs: bool,
    /// The hip-to-ankle height while rising, as a share of the standing leg's.
    pub air_tuck: f32,
    /// The hip-to-ankle height while falling fast, as a share of the standing leg's.
    pub air_hang: f32,
    /// The falling speed at which the legs are fully let down, metres per second.
    pub air_fall_speed_mps: f32,
    /// How far the toes point down with the legs let down, degrees.
    pub air_toe_drop_deg: f32,
    /// Dip the hips on landing from a jump or a short fall, by the speed of the fall.
    pub landing_dip: bool,
    /// How deep the landing dip goes per metre per second of falling speed past
    /// `landing_dip_min_speed_mps`, metres.
    pub landing_dip_per_mps: f32,
    /// The deepest landing dip, metres.
    pub landing_dip_max_m: f32,
    /// The falling speed below which a landing does not dip, metres per second.
    pub landing_dip_min_speed_mps: f32,
    /// How long after landing the dip is deepest, seconds; it recovers over a few times that.
    pub landing_dip_s: f32,
    /// How long a hit reaction (a stumble or a knock-back) holds the body on the animation after
    /// its act, seconds: the game keeps no state for the reaction's length.
    pub hit_reaction_hold_s: f32,
}

impl ProceduralBodyConfig {
    pub const fn new() -> Self {
        Self {
            mode: ProceduralMode::Full,
            blend_s: 0.35,
            blend_out_s: 0.2,
            counterbalance: 0.3,
            hips_follow_s: 0.2,
            min_hips_height: 0.5,
            pelvis_turn_limit_deg: 35.0,
            chest_turn_limit_deg: 70.0,
            lean_limit_deg: 60.0,
            neutralize_targets: true,
            plant_feet: true,
            step_threshold_m: 0.15,
            step_turn_deg: 35.0,
            step_duration_s: 0.3,
            step_height_m: 0.06,
            gait: true,
            gait_speed_mps: 0.3,
            walk_speed_mps: 1.5,
            walk_stride_m: 1.4,
            walk_duty: 0.62,
            walk_step_height_m: 0.08,
            run_speed_mps: 5.0,
            run_stride_m: 2.8,
            run_duty: 0.4,
            run_step_height_m: 0.14,
            crouch_gait: true,
            crouch_stride_share: 0.55,
            crouch_lift_share: 0.6,
            crouch_duty: 0.7,
            kneel: true,
            kneel_blend_s: 0.35,
            ground_casts: true,
            max_step_up_m: 0.5,
            max_step_down_m: 0.6,
            ankle_tilt_limit_deg: 25.0,
            reach_drop_s: 0.05,
            keep_out: true,
            chest_radius_m: 0.14,
            abdomen_radius_m: 0.13,
            hand_margin_m: 0.03,
            elbow_hints: true,
            elbow_hint_reach: 0.3,
            clavicles: true,
            clavicle_elevation_deg: 30.0,
            clavicle_depression_deg: 10.0,
            clavicle_protraction_deg: 25.0,
            clavicle_reach_start: 0.8,
            clavicle_follow_s: 0.1,
            air_legs: true,
            air_tuck: 0.72,
            air_hang: 0.94,
            air_fall_speed_mps: 4.0,
            air_toe_drop_deg: 20.0,
            landing_dip: true,
            landing_dip_per_mps: 0.02,
            landing_dip_max_m: 0.15,
            landing_dip_min_speed_mps: 1.5,
            landing_dip_s: 0.1,
            hit_reaction_hold_s: 1.2,
        }
    }

    /// Check the settings. Paths are relative to `controllers.body.procedural`.
    pub(crate) fn validate(&self) -> Result<(), Invalid> {
        let checks = [
            ("blend_s", self.blend_s, 0.0, 10.0),
            ("blend_out_s", self.blend_out_s, 0.0, 10.0),
            ("counterbalance", self.counterbalance, 0.0, 1.0),
            ("hips_follow_s", self.hips_follow_s, 0.0, 2.0),
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
            ("step_threshold_m", self.step_threshold_m, 0.01, 1.0),
            ("step_turn_deg", self.step_turn_deg, 1.0, 180.0),
            ("step_duration_s", self.step_duration_s, 0.05, 2.0),
            ("step_height_m", self.step_height_m, 0.0, 0.5),
            ("gait_speed_mps", self.gait_speed_mps, 0.05, 5.0),
            ("walk_speed_mps", self.walk_speed_mps, 0.1, 20.0),
            ("walk_stride_m", self.walk_stride_m, 0.1, 3.0),
            ("walk_duty", self.walk_duty, 0.2, 0.9),
            ("walk_step_height_m", self.walk_step_height_m, 0.0, 0.5),
            ("run_speed_mps", self.run_speed_mps, 0.1, 20.0),
            ("run_stride_m", self.run_stride_m, 0.1, 3.5),
            ("run_duty", self.run_duty, 0.2, 0.9),
            ("run_step_height_m", self.run_step_height_m, 0.0, 0.5),
            ("crouch_stride_share", self.crouch_stride_share, 0.1, 1.0),
            ("crouch_lift_share", self.crouch_lift_share, 0.0, 1.0),
            ("crouch_duty", self.crouch_duty, 0.2, 0.9),
            ("kneel_blend_s", self.kneel_blend_s, 0.0, 3.0),
            ("max_step_up_m", self.max_step_up_m, 0.0, 2.0),
            ("max_step_down_m", self.max_step_down_m, 0.0, 2.0),
            ("ankle_tilt_limit_deg", self.ankle_tilt_limit_deg, 0.0, 60.0),
            ("reach_drop_s", self.reach_drop_s, 0.0, 2.0),
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
            ("air_tuck", self.air_tuck, 0.4, 1.0),
            ("air_hang", self.air_hang, 0.4, 1.0),
            ("air_fall_speed_mps", self.air_fall_speed_mps, 0.1, 50.0),
            ("air_toe_drop_deg", self.air_toe_drop_deg, 0.0, 60.0),
            ("landing_dip_per_mps", self.landing_dip_per_mps, 0.0, 0.2),
            ("landing_dip_max_m", self.landing_dip_max_m, 0.0, 0.5),
            (
                "landing_dip_min_speed_mps",
                self.landing_dip_min_speed_mps,
                0.0,
                20.0,
            ),
            ("landing_dip_s", self.landing_dip_s, 0.02, 1.0),
            ("hit_reaction_hold_s", self.hit_reaction_hold_s, 0.0, 5.0),
        ];
        for (name, value, min, max) in checks {
            if !(min..=max).contains(&value) {
                return Err(Invalid(format!(
                    "body.procedural.{name} is {value}, outside [{min}, {max}]"
                )));
            }
        }
        if self.run_speed_mps <= self.walk_speed_mps {
            return Err(Invalid(format!(
                "body.procedural.run_speed_mps ({}) is not above walk_speed_mps ({})",
                self.run_speed_mps, self.walk_speed_mps
            )));
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
    /// Build the body under the player's head while Rico is on foot on the ground, standing and
    /// moving. Saved files from before the moving body name it `Body`.
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
