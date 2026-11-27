//! The IK body's tuning, read from the shipped `tuning/ik_body.toml` ([`IK_BODY`]): the gate's
//! blend and hand-overs, the FK pelvis's follower, the seeded pose, the climbing feet's casts and
//! braces, the feet's ground probes, the supports' prediction, and the HumanIK targets' gains.
//!
//! The feet's contact model is the player's settings' (`controllers.body.ik.contact`), not the
//! file's.

use body_rig::{brace::BraceTuning, fk::FkTuning};
use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The IK body's tuning file.
    pub static IK_BODY: IkBodyTuning = "ik_body.toml";
}

/// The IK body's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IkBodyTuning {
    pub gate: GateTuning,
    /// The FK pelvis's follower ([`body_rig::fk`]).
    pub fk: FkTuning,
    pub seed: SeedTuning,
    pub climb: ClimbTuning,
    /// The climbing feet's braces ([`body_rig::brace`]).
    pub brace: BraceTuning,
    pub probe: ProbeTuning,
    pub support: SupportTuning,
    pub effectors: EffectorsTuning,
}

impl Tuning for IkBodyTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.gate.validate()?;
        positive("fk.smooth_s", self.fk.smooth_s, 5.0)?;
        positive("fk.fallback_blend_s", self.fk.fallback_blend_s, 5.0)?;
        self.seed.validate()?;
        positive("climb.hip_cast_share", self.climb.hip_cast_share, 3.0)?;
        positive("climb.ankle_cast_share", self.climb.ankle_cast_share, 3.0)?;
        validate_brace(&self.brace)?;
        self.probe.validate()?;
        nonnegative("support.max_predict_s", self.support.max_predict_s, 1.0)?;
        self.effectors.validate()
    }
}

/// The solve gate's blend, hand-overs, and seeded pelvis.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateTuning {
    /// The IK pose's share of the pelvis's turn and the spine on the walk.
    pub(crate) walk_weight: f32,
    /// How long a hand-over between the IK pose and a clip takes, seconds.
    pub blend_s: f32,
    /// How long after a hand-over to the walk the IK waits before it takes the standing body back,
    /// seconds, once Rico stands still again.
    pub(crate) resume_s: f32,
    /// The furthest the pelvis is lowered so that the planted feet stay within the legs' reach,
    /// metres.
    pub max_pelvis_drop_m: f32,
    /// In the air, the share of the way from the estimate's legs to the light air pose.
    pub(crate) air_relax: f32,
    /// The toe joint's height over the ground, metres: a generic anthropometric figure.
    pub toe_height_m: f32,
    /// The pelvis position's weight from which the root follows the FK pelvis: past half way into
    /// the standing body, so the step commit keeps the root through the start of the blend.
    pub(crate) root_follow_weight: f32,
}

impl GateTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range("gate.walk_weight", self.walk_weight, 0.0, 1.0)?;
        positive("gate.blend_s", self.blend_s, 5.0)?;
        nonnegative("gate.resume_s", self.resume_s, 10.0)?;
        nonnegative("gate.max_pelvis_drop_m", self.max_pelvis_drop_m, 1.0)?;
        range("gate.air_relax", self.air_relax, 0.0, 1.0)?;
        positive("gate.toe_height_m", self.toe_height_m, 0.2)?;
        positive("gate.root_follow_weight", self.root_follow_weight, 1.0)
    }
}

/// The seeded pose's reach and the legs' light air pose.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedTuning {
    /// The share of the leg's straight length the pelvis's drop keeps the planted feet within, so
    /// the knee keeps a little bend to solve with.
    pub reach_share: f32,
    /// The light air pose's knee under the hip, as shares of the leg's length: down along the
    /// pelvis and ahead of it.
    pub(crate) air_knee_down: f32,
    pub(crate) air_knee_ahead: f32,
    /// The light air pose's ankle under the hip, as shares of the leg's length.
    pub(crate) air_ankle_down: f32,
    pub(crate) air_ankle_ahead: f32,
}

impl SeedTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("seed.reach_share", self.reach_share, 1.0)?;
        range("seed.air_knee_down", self.air_knee_down, 0.0, 1.0)?;
        range("seed.air_knee_ahead", self.air_knee_ahead, -1.0, 1.0)?;
        range("seed.air_ankle_down", self.air_ankle_down, 0.0, 1.0)?;
        range("seed.air_ankle_ahead", self.air_ankle_ahead, -1.0, 1.0)
    }
}

/// The climbing feet's casts.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimbTuning {
    /// How far the cast from the hip along the facing reaches, as a share of the leg's length.
    pub(crate) hip_cast_share: f32,
    /// How far the cast from the ankle forward and down reaches, as a share of the leg's length.
    pub(crate) ankle_cast_share: f32,
}

/// The feet's ground probes.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeTuning {
    /// How far above the height asked about a probe starts, metres: a step up.
    pub(crate) above_m: f32,
    /// How far below it a probe reaches, metres: a step down; past it, the foot is unsupported.
    pub below_m: f32,
    /// A request this close to an answer's point is answered by it, metres.
    pub(crate) same_point_m: f32,
    /// ...and this close to its height, metres.
    pub(crate) same_height_m: f32,
}

impl ProbeTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("probe.above_m", self.above_m, 3.0)?;
        nonnegative("probe.below_m", self.below_m, 3.0)?;
        positive(
            "probe.above_m + probe.below_m",
            self.above_m + self.below_m,
            6.0,
        )?;
        nonnegative("probe.same_point_m", self.same_point_m, 0.5)?;
        nonnegative("probe.same_height_m", self.same_height_m, 2.0)
    }
}

/// The feet's moving supports.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportTuning {
    /// The furthest a snapshot is carried forward by its velocities, seconds: a few ticks.
    pub(crate) max_predict_s: f32,
}

/// The HumanIK targets' gains at full weight.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectorsTuning {
    /// The hips' position target.
    pub hips: Gains,
    /// The hips effector's rotation reach: the zero rotation offset that holds the seeded pelvis's
    /// orientation through the solve.
    pub(crate) hips_rotation_reach: f32,
    /// How firmly the hips resist being displaced by other effectors' pull.
    pub(crate) hips_resist: f32,
    /// The ankle and toe targets, planted or braced. The pull is how much reaching a foot's target
    /// drags the rest of the body after it.
    pub foot: Gains,
    /// The knee hints: weak, a direction rather than a place.
    pub(crate) knee_hint: Gains,
    /// In the air, the ankle targets.
    pub(crate) air_foot: Gains,
}

impl EffectorsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        for (name, gains) in [
            ("effectors.hips", self.hips),
            ("effectors.foot", self.foot),
            ("effectors.knee_hint", self.knee_hint),
            ("effectors.air_foot", self.air_foot),
        ] {
            range(&format!("{name}.reach"), gains.reach, 0.0, 1.0)?;
            range(&format!("{name}.pull"), gains.pull, 0.0, 1.0)?;
        }
        range(
            "effectors.hips_rotation_reach",
            self.hips_rotation_reach,
            0.0,
            1.0,
        )?;
        range("effectors.hips_resist", self.hips_resist, 0.0, 1.0)
    }
}

/// A target's reach and pull at full weight.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gains {
    pub reach: f32,
    pub pull: f32,
}

/// Check the braces: the shares ordered so a fresh brace is not let go at once, and the move's
/// time a divisor.
fn validate_brace(brace: &BraceTuning) -> Result<(), Invalid> {
    positive("brace.reach_share", brace.reach_share, 2.0)?;
    range(
        "brace.acquire_share",
        brace.acquire_share,
        0.0,
        brace.reach_share,
    )?;
    range("brace.min_share", brace.min_share, 0.0, brace.acquire_share)?;
    range("brace.min_facing", brace.min_facing, -1.0, 1.0)?;
    nonnegative("brace.knee_m", brace.knee_m, 1.0)?;
    positive("brace.move_s", brace.move_s, 5.0)?;
    nonnegative("brace.arc_m", brace.arc_m, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(IK_BODY.get().validate(), Ok(()));
    }
}
