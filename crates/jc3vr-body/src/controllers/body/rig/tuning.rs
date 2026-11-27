//! The body rig's tuning, read from the shipped `tuning/rig.toml` ([`RIG`]): the pure core's
//! ([`CoreTuning`]), the step's walk, the torso readout, the head probe, the root's warp test and
//! jump trace, the ledger's obstruction, and the push-back.
//!
//! The pelvis solver's, the step commit's, and the yaw's tuning are the player's settings
//! ([`crate::controllers::body::rig::RigConfig`]), as is the feet's contact tuning, not the file's.
//! A tracking recording carries those settings, so `tools/tracking-synth` replays them as
//! recorded; this file is not recorded, and a replay reads the repository's current `core` table.

use body_rig::{CoreTuning, height::WINDOW};
use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::tuning::Millis;

jc3vr_core::tuning_file! {
    /// The body rig's tuning file.
    pub static RIG: BodyRigTuning = "rig.toml";
}

/// The body rig's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyRigTuning {
    /// The pure core's tuning (`body-rig`), which the offline evaluator reads too.
    pub core: CoreTuning,
    pub grounded: GroundedFrameTuning,
    pub torso: TorsoTuning,
    pub probe: ProbeTuning,
    pub root: RootTuning,
    pub trace: TraceTuning,
    pub ledger: LedgerTuning,
    pub pushback: PushbackTuning,
}

impl Tuning for BodyRigTuning {
    fn validate(&self) -> Result<(), Invalid> {
        validate_core(&self.core)?;
        self.grounded.validate()?;
        range(
            "torso.hip_spacing_share",
            self.torso.hip_spacing_share,
            0.0,
            1.0,
        )?;
        range(
            "torso.chest_limit_deg",
            self.torso.chest_limit_deg,
            0.0,
            180.0,
        )?;
        self.probe.validate()?;
        self.root.validate()?;
        positive("trace.jump_m", self.trace.jump_m, 1.0)?;
        range(
            "ledger.obstructed_share",
            self.ledger.obstructed_share,
            0.0,
            1.0,
        )?;
        self.pushback.validate()
    }
}

/// The step's walk and the grounded rig's restart.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundedFrameTuning {
    /// How far a committed step pushes the move stick for the game's animation, `0..=1`: a walk.
    /// The velocity is the drive's whatever this is; it only picks the clip.
    pub(crate) walk_stick: f32,
    /// A step closer than this to arriving shuffles under the drive rather than walking, metres.
    pub(crate) walk_gap_m: f32,
    /// A spell off the ground longer than this starts the rig again facing the head.
    pub resume_after_ms: Millis,
}

impl GroundedFrameTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range("grounded.walk_stick", self.walk_stick, 0.0, 1.0)?;
        nonnegative("grounded.walk_gap_m", self.walk_gap_m, 1.0)?;
        self.resume_after_ms
            .validate("grounded.resume_after_ms", 10_000)
    }
}

/// The torso readout's shape.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TorsoTuning {
    /// The distance between the hip joints as a share of the distance between the shoulder joints:
    /// a generic anthropometric figure.
    pub(crate) hip_spacing_share: f32,
    /// The furthest the chest is turned from the character's facing, degrees.
    pub(crate) chest_limit_deg: f32,
}

/// The head-through-geometry probe and its fade.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeTuning {
    /// How far above the eyes the terrain check starts, metres: deeper than the full fade, and
    /// short enough that the heightfield over a tunnel or a bunker is never in reach.
    pub(crate) terrain_reach_m: f32,
    /// How deep past the obstruction the head may go before the view starts to darken, metres.
    pub(crate) head_fade_start_m: f32,
    /// How deep past the obstruction the view is fully dark, metres.
    pub(crate) head_fade_full_m: f32,
    /// How far to either side of the eye centre the outer rays of the fan run, metres: about the
    /// head's half-width.
    pub(crate) fan_half_width_m: f32,
}

impl ProbeTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("probe.terrain_reach_m", self.terrain_reach_m, 10.0)?;
        nonnegative("probe.head_fade_start_m", self.head_fade_start_m, 1.0)?;
        range(
            "probe.head_fade_full_m",
            self.head_fade_full_m,
            self.head_fade_start_m,
            2.0,
        )?;
        nonnegative("probe.fan_half_width_m", self.fan_half_width_m, 0.5)
    }
}

/// The root's seat exits, headroom, and warp test.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootTuning {
    /// The gap at a seat exit past which the head is put back over the body instead of the body
    /// stepping to it, world metres.
    pub(crate) exit_rebase_m: f32,
    /// Where the headroom ray starts above the feet, metres: above a step the character stands on.
    pub(crate) headroom_ray_start_m: f32,
    /// The shortest single-tick jump of the body, metres, that counts as a teleport the game made
    /// without announcing it.
    pub(crate) warp_min_m: f32,
    /// How many times what the body's speed covers in the tick a jump must also exceed to count as
    /// a teleport, so that fast vehicles and dives never trip it.
    pub(crate) warp_speed_factor: f32,
}

impl RootTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("root.exit_rebase_m", self.exit_rebase_m, 10.0)?;
        nonnegative("root.headroom_ray_start_m", self.headroom_ray_start_m, 2.0)?;
        positive("root.warp_min_m", self.warp_min_m, 1_000.0)?;
        positive("root.warp_speed_factor", self.warp_speed_factor, 100.0)
    }
}

/// The root trace's jump test.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceTuning {
    /// How far the root may move beyond what the wanted velocity explains in a tick before the
    /// move counts as a jump, metres: above a stair's pop and the proxy's own settling.
    pub jump_m: f32,
}

/// The step drive's ledger.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerTuning {
    /// The share of the requested drive below which the credited drive means an obstruction.
    pub(crate) obstructed_share: f32,
}

/// The push-back at a wall the body cannot pass.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushbackTuning {
    /// How far past the root the view may go while the player's step drive is obstructed, metres.
    pub(crate) obstructed_m: f32,
    /// How far while the animation carries the body and the drive is suspended, metres: only a
    /// runaway gap.
    pub authored_m: f32,
    /// How far while climbing, metres: about an arm's-length lean.
    pub reach_m: f32,
    /// The smallest gap that counts as obstructed, metres.
    pub(crate) min_gap_m: f32,
    /// The most the view is slid back in one episode, metres; past it the view fades.
    pub(crate) debt_cap_m: f32,
    /// How far past the threshold, with the cap spent, the fade reaches black, metres.
    pub(crate) fade_span_m: f32,
    /// How long the gap must stay under the calm share of the threshold for the episode to end,
    /// seconds.
    pub(crate) reset_s: f32,
    /// The share of the threshold the gap must stay under for the episode to end.
    pub(crate) calm_share: f32,
}

impl PushbackTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("pushback.obstructed_m", self.obstructed_m, 2.0)?;
        positive("pushback.authored_m", self.authored_m, 2.0)?;
        positive("pushback.reach_m", self.reach_m, 2.0)?;
        nonnegative("pushback.min_gap_m", self.min_gap_m, 1.0)?;
        positive("pushback.debt_cap_m", self.debt_cap_m, 2.0)?;
        positive("pushback.fade_span_m", self.fade_span_m, 2.0)?;
        nonnegative("pushback.reset_s", self.reset_s, 5.0)?;
        range("pushback.calm_share", self.calm_share, 0.0, 1.0)
    }
}

/// Check the pure core's tuning: the divisors positive, the shares in range, and the samples the
/// standing height needs within the ones it holds.
fn validate_core(core: &CoreTuning) -> Result<(), Invalid> {
    let grounded = &core.grounded;
    range(
        "core.grounded.level_gaze_deg",
        grounded.level_gaze_deg,
        0.0,
        90.0,
    )?;
    nonnegative(
        "core.grounded.head_speed_smoothing_s",
        grounded.head_speed_smoothing_s,
        5.0,
    )?;
    let height = &core.height;
    positive("core.height.sample_s", height.sample_s, 5.0)?;
    if !(1..=WINDOW).contains(&height.min_samples) {
        return Err(Invalid(format!(
            "core.height.min_samples must be in [1, {WINDOW}], got {}",
            height.min_samples
        )));
    }
    nonnegative("core.height.max_drift_m", height.max_drift_m, 1.0)?;
    range("core.height.percentile", height.percentile, 0.0, 1.0)?;
    positive("core.yaw.hand_full_m", core.yaw.hand_full_m, 2.0)?;
    nonnegative("core.yaw.turning_deg_s", core.yaw.turning_deg_s, 360.0)?;
    let contact = &core.contact;
    nonnegative(
        "core.contact.velocity_smoothing_s",
        contact.velocity_smoothing_s,
        5.0,
    )?;
    nonnegative("core.contact.heel_m", contact.heel_m, 0.5)?;
    range(
        "core.contact.follow_up_share",
        contact.follow_up_share,
        0.0,
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(RIG.get().validate(), Ok(()));
    }
}
