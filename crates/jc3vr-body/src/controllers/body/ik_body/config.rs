//! The IK body's settings, `controllers.body.ik` (`docs/mod/body/ik-body.md`).

use body_rig::contact::ContactTuning;
use jc3vr_core::config::Invalid;
use serde::{Deserialize, Serialize};

/// The IK-first on-foot body's settings: whether it gives the firm ragdoll its muscle target, and
/// the feet's contact tuning, which `tracking-synth rig-eval --contact` replays.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IkBodyConfig {
    /// Give the firm ragdoll the drawn upper body and the IK body's legs as its muscle target.
    pub ragdoll_target: bool,
    pub contact: ContactTuning,
}

impl IkBodyConfig {
    pub const fn new() -> Self {
        Self {
            ragdoll_target: true,
            contact: ContactTuning::new(),
        }
    }

    /// Check the settings. Paths are relative to `controllers.body.ik`.
    pub fn validate(&self) -> Result<(), Invalid> {
        let c = &self.contact;
        let checks = [
            ("contact.step_s", c.step_s, 0.05, 2.0),
            ("contact.arc_m", c.arc_m, 0.0, 0.4),
            ("contact.reach_share", c.reach_share, 0.5, 1.5),
            ("contact.reach_drop_m", c.reach_drop_m, 0.0, 0.5),
            ("contact.twist_deg", c.twist_deg, 5.0, 120.0),
            ("contact.twist_hold_s", c.twist_hold_s, 0.0, 3.0),
            ("contact.com_margin_m", c.com_margin_m, 0.0, 0.5),
            ("contact.com_neck_share", c.com_neck_share, 0.0, 1.0),
            ("contact.root_tolerance_m", c.root_tolerance_m, 0.0, 2.0),
            ("contact.tidy_m", c.tidy_m, 0.0, 1.0),
            ("contact.tidy_deg", c.tidy_deg, 0.0, 120.0),
            ("contact.tidy_s", c.tidy_s, 0.0, 30.0),
            ("contact.lead", c.lead, 0.0, 2.0),
            ("contact.max_step_m", c.max_step_m, 0.1, 2.0),
            ("contact.walk_speed_mps", c.walk_speed_mps, 0.0, 5.0),
            ("contact.outrun_m", c.outrun_m, 0.0, 1.0),
            ("contact.foot_m", c.foot_m, 0.0, 0.5),
            ("contact.landing_wait_s", c.landing_wait_s, 0.0, 2.0),
        ];
        for (name, value, min, max) in checks {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(Invalid(format!(
                    "body.ik.{name} must be finite and in [{min}, {max}], got {value}"
                )));
            }
        }
        Ok(())
    }
}

impl Default for IkBodyConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_validate_and_a_bad_step_does_not() {
        assert_eq!(IkBodyConfig::new().validate(), Ok(()));
        let mut bad = IkBodyConfig::new();
        bad.contact.step_s = f32::NAN;
        assert!(bad.validate().unwrap_err().0.contains("contact.step_s"));
    }
}
