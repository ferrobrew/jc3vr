//! The body rig's settings, under `controllers.body.rig` (`docs/mod/body/rig.md`).

use body_rig::{PelvisTuning, StepTuning, YawTuning};
use jc3vr_core::config::engine::Invalid;
use serde::{Deserialize, Serialize};

/// The body rig's step switch and tuning. The switch defaults on; the tuning is the pure core's
/// (`body-rig`), set from the offline evaluation of a standing session.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigConfig {
    /// The step drive: Rico walking after the player's head on foot, seated or standing.
    pub steps: StepsConfig,
    /// The pelvis solver's tuning: the lean's radius and reach, and the bend limits.
    pub pelvis: PelvisTuning,
    /// The step commit's tuning: the threshold, the arrival, and the drive.
    pub step: StepTuning,
    /// The yaw's tuning: the chest's clamp and follow, when the legs turn, the slew, and the
    /// hands' pull on the chest.
    pub yaw: YawTuning,
}

impl RigConfig {
    pub const fn new() -> Self {
        Self {
            steps: StepsConfig::new(),
            pelvis: PelvisTuning::new(),
            step: StepTuning::new(),
            yaw: YawTuning::new(),
        }
    }

    /// The pure core's tuning.
    pub fn tuning(&self) -> body_rig::RigTuning {
        body_rig::RigTuning {
            pelvis: self.pelvis,
            step: self.step,
            yaw: self.yaw,
        }
    }

    pub fn validate(&self) -> Result<(), Invalid> {
        let p = &self.pelvis;
        let s = &self.step;
        let y = &self.yaw;
        let e = &y.estimate;
        let checks = [
            ("pelvis.lean_radius_m", p.lean_radius_m, 0.0, 1.0),
            ("pelvis.back_radius_m", p.back_radius_m, 0.0, 1.0),
            ("pelvis.max_lean_m", p.max_lean_m, 0.05, 1.5),
            ("pelvis.forward_bend_deg", p.forward_bend_deg, 1.0, 89.0),
            ("pelvis.side_bend_deg", p.side_bend_deg, 1.0, 89.0),
            ("pelvis.back_bend_deg", p.back_bend_deg, 1.0, 89.0),
            ("pelvis.hand_reach_m", p.hand_reach_m, 0.0, 1.5),
            ("pelvis.hand_lean_m", p.hand_lean_m, 0.0, 0.5),
            ("step.commit_m", s.commit_m, 0.0, 1.0),
            ("step.arrive_m", s.arrive_m, 0.005, 0.5),
            ("step.response_s", s.response_s, 0.02, 5.0),
            ("step.max_speed_mps", s.max_speed_mps, 0.05, 6.0),
            ("step.settle_speed_mps", s.settle_speed_mps, 0.0, 2.0),
            ("step.settle_done_m", s.settle_done_m, 0.0, 0.5),
            ("step.settle_s", s.settle_s, 0.0, 10.0),
            ("step.give_up_s", s.give_up_s, 0.1, 30.0),
            ("step.drift_min_m", s.drift_min_m, 0.0, 0.5),
            ("step.drift_after_s", s.drift_after_s, 0.0, 30.0),
            ("step.drift_steady_mps", s.drift_steady_mps, 0.0, 2.0),
            ("step.drift_speed_mps", s.drift_speed_mps, 0.0, 2.0),
            ("step.follow_response_s", s.follow_response_s, 0.02, 5.0),
            ("yaw.chest_clamp_deg", y.chest_clamp_deg, 0.0, 120.0),
            ("yaw.chest_follow_s", y.chest_follow_s, 0.0, 5.0),
            ("yaw.turn_deg", y.turn_deg, 0.0, 180.0),
            ("yaw.sustain_deg", y.sustain_deg, 0.0, 180.0),
            ("yaw.sustain_s", y.sustain_s, 0.0, 30.0),
            ("yaw.moving_deadzone_deg", y.moving_deadzone_deg, 0.0, 120.0),
            ("yaw.moving_sustain_s", y.moving_sustain_s, 0.0, 10.0),
            ("yaw.settle_deg", y.settle_deg, 0.0, 60.0),
            ("yaw.slew_deg_s", y.slew_deg_s, 1.0, 2000.0),
            ("yaw.steady_deg", y.steady_deg, 0.0, 90.0),
            ("yaw.steady_s", y.steady_s, 0.0, 60.0),
            ("yaw.steady_settle_deg", y.steady_settle_deg, 0.0, 180.0),
            ("yaw.steady_slew_deg_s", y.steady_slew_deg_s, 1.0, 2000.0),
            ("yaw.look_down_deg", y.look_down_deg, 0.0, 90.0),
            ("yaw.look_down_full_deg", y.look_down_full_deg, 0.0, 90.0),
            ("yaw.hand_bias", y.hand_bias, 0.0, 1.0),
            ("yaw.hand_min_m", y.hand_min_m, 0.0, 1.5),
            ("yaw.estimate.blend_s", e.blend_s, 0.0, 10.0),
            ("yaw.estimate.smooth_s", e.smooth_s, 0.0, 5.0),
            ("yaw.estimate.deadband_deg", e.deadband_deg, 0.0, 90.0),
            ("yaw.estimate.slew_deg_s", e.slew_deg_s, 1.0, 2000.0),
            ("yaw.estimate.centre_deg_s", e.centre_deg_s, 0.0, 360.0),
            ("yaw.estimate.still_deg_s", e.still_deg_s, 0.0, 360.0),
            ("yaw.estimate.still_s", e.still_s, 0.0, 30.0),
            ("yaw.estimate.chest_weight", e.chest_weight, 0.0, 1.0),
            ("yaw.estimate.stick_blend_s", e.stick_blend_s, 0.0, 10.0),
            ("yaw.estimate.fresh_s", e.fresh_s, 0.0, 5.0),
            ("yaw.estimate.stale_s", e.stale_s, 0.0, 10.0),
        ];
        for (name, value, min, max) in checks {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(Invalid(format!(
                    "body.rig.{name} must be finite and in [{min}, {max}], got {value}"
                )));
            }
        }
        if y.look_down_full_deg < y.look_down_deg {
            return Err(Invalid(format!(
                "body.rig.yaw.look_down_full_deg ({}) is below look_down_deg ({})",
                y.look_down_full_deg, y.look_down_deg
            )));
        }
        if e.stale_s <= e.fresh_s {
            return Err(Invalid(format!(
                "body.rig.yaw.estimate.stale_s ({}) is not above fresh_s ({})",
                e.stale_s, e.fresh_s
            )));
        }
        if s.settle_done_m > s.arrive_m {
            return Err(Invalid(format!(
                "body.rig.step.settle_done_m ({}) is above arrive_m ({})",
                s.settle_done_m, s.arrive_m
            )));
        }
        Ok(())
    }
}

impl Default for RigConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The step drive.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StepsConfig {
    pub enabled: bool,
}

impl StepsConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for StepsConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_validate() {
        assert_eq!(RigConfig::new().validate(), Ok(()));
        let mut bad = RigConfig::new();
        bad.step.response_s = f32::NAN;
        assert!(bad.validate().is_err());
        bad = RigConfig::new();
        bad.yaw.hand_bias = 2.0;
        assert!(
            bad.validate()
                .unwrap_err()
                .0
                .contains("body.rig.yaw.hand_bias")
        );
        bad = RigConfig::new();
        bad.yaw.estimate.stale_s = bad.yaw.estimate.fresh_s;
        assert!(bad.validate().unwrap_err().0.contains("stale_s"));
    }

    #[test]
    fn a_saved_file_from_the_procedural_rig_still_loads() {
        let saved = "[chase]\nenabled = false\n[lean]\nenabled = true\n[legs]\nstep_s = 0.3\n\
                     [pelvis]\nlean_radius_m = 0.25\n";
        let config: RigConfig = toml::from_str(saved).expect("old keys are ignored");
        assert_eq!(config.pelvis.lean_radius_m, 0.25);
        assert!(config.steps.enabled);
    }
}
