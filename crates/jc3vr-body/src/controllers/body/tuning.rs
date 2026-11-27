//! The body's tuning, read from the shipped `tuning/body.toml` ([`BODY`]): the calibration flow's
//! capture, the firm ragdoll's blends, and the learned estimator's submission rate. The procedural
//! body and the IK body keep their own files, beside their code.

use jc3vr_core::{
    config::{Invalid, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::tuning::Millis;

jc3vr_core::tuning_file! {
    /// The body's tuning file.
    pub static BODY: BodyTuning = "body.toml";
}

/// The body's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyTuning {
    pub calibration: CalibrationTuning,
    pub firm_ragdoll: FirmRagdollTuning,
    pub estimator: EstimatorTuning,
}

impl Tuning for BodyTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let c = &self.calibration;
        c.countdown_ms
            .validate("calibration.countdown_ms", 30_000)?;
        c.sample_window_ms
            .validate("calibration.sample_window_ms", 10_000)?;
        if !(1..=1_000).contains(&c.min_samples) {
            return Err(Invalid(format!(
                "calibration.min_samples must be in [1, 1000], got {}",
                c.min_samples
            )));
        }
        bounds("calibration.eye_height_m", c.eye_height_m, 10.0)?;
        bounds(
            "calibration.seated_eye_height_m",
            c.seated_eye_height_m,
            10.0,
        )?;
        bounds("calibration.arm_span_m", c.arm_span_m, 10.0)?;
        range(
            "calibration.arm_span_min_ratio",
            c.arm_span_min_ratio,
            0.1,
            1.0,
        )?;
        range(
            "calibration.arm_span_flag_ratio",
            c.arm_span_flag_ratio,
            1.0,
            3.0,
        )?;
        let f = &self.firm_ragdoll;
        positive("firm_ragdoll.blend_in_s", f.blend_in_s, 5.0)?;
        positive("firm_ragdoll.blend_out_s", f.blend_out_s, 5.0)?;
        let e = &self.estimator;
        positive("estimator.submit_hz", e.submit_hz, 120.0)?;
        e.submit_early_ms.validate("estimator.submit_early_ms", 100)
    }
}

/// The calibration flow's capture ([`crate::controllers::body::calibration::Calibration`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationTuning {
    /// How long the flow waits after MEASURE is selected, so that the player can take the pose.
    pub countdown_ms: Millis,
    /// How long the flow samples once the countdown ends.
    pub sample_window_ms: Millis,
    /// The fewest samples a capture needs to count.
    pub min_samples: u32,
    /// The standing eye heights a calibration accepts, metres, as `[lowest, highest]`. Outside
    /// this, the player is most likely seated or the runtime's floor is wrong.
    pub(crate) eye_height_m: (f32, f32),
    /// The seated eye heights a calibration accepts, metres: from a low sofa to a tall stool.
    /// Above this, the player is most likely standing.
    pub(crate) seated_eye_height_m: (f32, f32),
    /// The arm spans a calibration accepts, metres, whatever the height.
    pub(crate) arm_span_m: (f32, f32),
    /// The shortest arm span a calibration accepts, as a fraction of the height. An adult's span
    /// is within a few percent of their height, so a shorter one means the arms were not spread.
    /// There is no upper bound relative to the height: the grip span cannot exceed the arms, so a
    /// span well over the height means the height is low (the eye height was measured seated), not
    /// the arms long, and the review flags it instead (`arm_span_flag_ratio`).
    pub(crate) arm_span_min_ratio: f32,
    /// The arm span, as a fraction of the height, above which the review flags the two as out of
    /// proportion.
    pub(crate) arm_span_flag_ratio: f32,
}

/// The firm ragdoll's blends ([`crate::controllers::body::firm_ragdoll`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirmRagdollTuning {
    /// How long the physics takes to come in, seconds.
    pub(crate) blend_in_s: f32,
    /// How long the physics takes to go out, seconds.
    pub(crate) blend_out_s: f32,
}

/// The learned estimator's submissions ([`crate::controllers::body::estimator`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstimatorTuning {
    /// How many windows a second go to the GPU.
    pub submit_hz: f32,
    /// How early a frame may take a submission's due time: a frame's jitter.
    pub(crate) submit_early_ms: Millis,
}

/// Check that `value` is a `[lowest, highest]` pair of positive lengths at most `max`, in order.
fn bounds(name: &str, value: (f32, f32), max: f32) -> Result<(), Invalid> {
    positive(&format!("{name}[0]"), value.0, max)?;
    positive(&format!("{name}[1]"), value.1, max)?;
    if value.0 >= value.1 {
        return Err(Invalid(format!(
            "{name} must be ordered lowest first, got [{}, {}]",
            value.0, value.1
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(BODY.get().validate(), Ok(()));
    }
}
