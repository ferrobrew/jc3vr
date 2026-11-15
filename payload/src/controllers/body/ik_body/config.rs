//! The IK body's settings, `controllers.body.ik` (`docs/mod/body/ik-body.md`).

use body_rig::{brace::BraceTuning, contact::ContactTuning, fk::FkTuning};
use serde::{Deserialize, Serialize};

use crate::config::validate::Invalid;

/// The IK-first on-foot body: its switch, its blend, how firmly it holds HumanIK's effectors, and
/// the tuning of the FK pelvis and the feet.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IkBodyConfig {
    /// Pose Rico's lower body from the learned estimate while he stands on foot and in the air.
    pub enabled: bool,
    /// Draw the airborne body from the estimate as well as the standing one.
    pub airborne: bool,
    /// While the hands climb, brace each foot on the face in front of it, or let it hang relaxed.
    pub climb_feet: bool,
    /// The IK pose's share of the pelvis's turn and the spine on the walk.
    pub walk_weight: f32,
    /// How long a hand-over between the IK pose and a clip takes, seconds.
    pub blend_s: f32,
    /// How long after a hand-over to the walk the IK waits before it takes the standing body back,
    /// seconds, once Rico stands still again.
    pub resume_s: f32,
    /// The hips effector's translation reach at full weight.
    pub hips_reach: f32,
    /// The hips effector's rotation reach at full weight: the zero rotation offset that holds the
    /// seeded pelvis's orientation through the solve.
    pub hips_rotation_reach: f32,
    /// How firmly the hips resist being displaced by other effectors' pull.
    pub hips_resist: f32,
    /// The ankle and toe effectors' translation reach at full weight.
    pub foot_reach: f32,
    /// How much reaching a foot's target drags the rest of the body after it.
    pub foot_pull: f32,
    /// The knee hints' translation reach at full weight: weak, a direction rather than a place.
    pub knee_hint_reach: f32,
    /// The furthest the pelvis is lowered so that the planted feet stay within the legs' reach,
    /// metres.
    pub max_pelvis_drop_m: f32,
    /// In the air, the share of the way from the estimate's legs to the light air pose.
    pub air_relax: f32,
    /// In the air, the ankle effectors' translation reach at full weight.
    pub air_foot_reach: f32,
    pub fk: FkTuning,
    pub contact: ContactTuning,
    pub brace: BraceTuning,
}

impl IkBodyConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            airborne: true,
            climb_feet: true,
            walk_weight: 0.2,
            blend_s: 0.25,
            resume_s: 0.4,
            hips_reach: 1.0,
            hips_rotation_reach: 1.0,
            hips_resist: 1.0,
            foot_reach: 1.0,
            foot_pull: 0.0,
            knee_hint_reach: 0.2,
            max_pelvis_drop_m: 0.2,
            air_relax: 0.5,
            air_foot_reach: 0.6,
            fk: FkTuning::new(),
            contact: ContactTuning::new(),
            brace: BraceTuning::new(),
        }
    }

    /// Check the settings. Paths are relative to `controllers.body.ik`.
    pub(crate) fn validate(&self) -> Result<(), Invalid> {
        let c = &self.contact;
        let b = &self.brace;
        let checks = [
            ("walk_weight", self.walk_weight, 0.0, 1.0),
            ("blend_s", self.blend_s, 0.0, 5.0),
            ("resume_s", self.resume_s, 0.0, 10.0),
            ("hips_reach", self.hips_reach, 0.0, 1.0),
            ("hips_rotation_reach", self.hips_rotation_reach, 0.0, 1.0),
            ("hips_resist", self.hips_resist, 0.0, 1.0),
            ("foot_reach", self.foot_reach, 0.0, 1.0),
            ("foot_pull", self.foot_pull, 0.0, 1.0),
            ("knee_hint_reach", self.knee_hint_reach, 0.0, 1.0),
            ("max_pelvis_drop_m", self.max_pelvis_drop_m, 0.0, 0.6),
            ("air_relax", self.air_relax, 0.0, 1.0),
            ("air_foot_reach", self.air_foot_reach, 0.0, 1.0),
            ("fk.smooth_s", self.fk.smooth_s, 0.0, 2.0),
            ("fk.fallback_blend_s", self.fk.fallback_blend_s, 0.0, 5.0),
            ("contact.step_s", c.step_s, 0.05, 2.0),
            ("contact.arc_m", c.arc_m, 0.0, 0.4),
            ("contact.reach_share", c.reach_share, 0.5, 1.5),
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
            ("brace.reach_share", b.reach_share, 0.5, 1.2),
            ("brace.acquire_share", b.acquire_share, 0.3, 1.2),
            ("brace.min_share", b.min_share, 0.0, 1.0),
            ("brace.min_facing", b.min_facing, -1.0, 1.0),
            ("brace.knee_m", b.knee_m, 0.0, 0.5),
            ("brace.move_s", b.move_s, 0.0, 2.0),
            ("brace.arc_m", b.arc_m, 0.0, 0.4),
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
