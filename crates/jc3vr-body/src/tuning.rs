//! The body crate's shared tuning: the [`Millis`] form its files write a wall-clock duration in, and
//! the freshness windows ([`FRESHNESS`]), which say how long each publication the body's subsystems
//! read from one another stays current. The windows are gathered in one file, rather than beside
//! each subsystem's other tuning, so that every staleness rule the crate applies is listed in one
//! place.

use std::time::Duration;

use jc3vr_core::{
    config::engine::{Invalid, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The body's freshness windows.
    pub static FRESHNESS: FreshnessTuning = "freshness.toml";
}

/// A wall-clock duration in whole milliseconds, as the tuning files write one.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Millis(pub u64);

impl Millis {
    pub const fn duration(self) -> Duration {
        Duration::from_millis(self.0)
    }

    /// Check that the duration is above zero and at most `max_ms`.
    pub fn validate(self, name: &str, max_ms: u64) -> Result<(), Invalid> {
        if self.0 == 0 || self.0 > max_ms {
            return Err(Invalid(format!(
                "{name} must be in [1, {max_ms}] milliseconds, got {}",
                self.0
            )));
        }
        Ok(())
    }
}

/// The freshness file's root: how long each publication stays current for its readers, by the
/// subsystem that reads it.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshnessTuning {
    pub aim: AimFreshness,
    pub body: BodyFreshness,
    pub rig: RigFreshness,
    pub hands: HandsFreshness,
    pub fingers: FingersFreshness,
    pub held: HeldFreshness,
    pub locomotion: LocomotionFreshness,
    pub roomscale: RoomscaleFreshness,
    pub facts: FactsFreshness,
    pub hooks: HooksFreshness,
    pub physics: PhysicsFreshness,
}

impl Tuning for FreshnessTuning {
    fn validate(&self) -> Result<(), Invalid> {
        const MAX_MS: u64 = 10_000;
        let windows = [
            ("aim.gun_ms", self.aim.gun_ms),
            ("body.unarmed_ms", self.body.unarmed_ms),
            ("body.estimator_eyes_ms", self.body.estimator_eyes_ms),
            ("body.estimate_ms", self.body.estimate_ms),
            ("body.ik_support_ms", self.body.ik_support_ms),
            ("body.ik_fk_ms", self.body.ik_fk_ms),
            ("body.ik_gate_ms", self.body.ik_gate_ms),
            ("body.procedural_gate_ms", self.body.procedural_gate_ms),
            ("body.procedural_local_ms", self.body.procedural_local_ms),
            (
                "body.firm_ragdoll_source_ms",
                self.body.firm_ragdoll_source_ms,
            ),
            ("rig.grounded_ms", self.rig.grounded_ms),
            ("rig.skeleton_ms", self.rig.skeleton_ms),
            ("rig.probe_ms", self.rig.probe_ms),
            ("rig.root_trace_ms", self.rig.root_trace_ms),
            ("rig.root_frame_gap_ms", self.rig.root_frame_gap_ms),
            ("hands.anchor_ms", self.hands.anchor_ms),
            ("hands.hang_gap_ms", self.hands.hang_gap_ms),
            ("hands.measured_arms_ms", self.hands.measured_arms_ms),
            ("hands.presentation_ms", self.hands.presentation_ms),
            ("hands.grab_status_ms", self.hands.grab_status_ms),
            ("held.guns_ms", self.held.guns_ms),
            (
                "locomotion.movement_task_ms",
                self.locomotion.movement_task_ms,
            ),
            ("locomotion.strokes_ms", self.locomotion.strokes_ms),
            ("roomscale.frame_gap_ms", self.roomscale.frame_gap_ms),
            ("facts.driving_ms", self.facts.driving_ms),
            ("hooks.layer_mask_ms", self.hooks.layer_mask_ms),
            (
                "hooks.body_ik_animation_gap_ms",
                self.hooks.body_ik_animation_gap_ms,
            ),
            (
                "hooks.parachute_align_gap_ms",
                self.hooks.parachute_align_gap_ms,
            ),
        ];
        for (name, window) in windows {
            window.validate(name, MAX_MS)?;
        }
        let f = &self.fingers;
        positive("fingers.grip_min_s", f.grip_min_s, 1.0)?;
        range("fingers.grip_max_s", f.grip_max_s, f.grip_min_s, 1.0)?;
        positive("fingers.grip_ticks", f.grip_ticks, 100.0)?;
        if !(1..=1_000).contains(&self.physics.stale_steps) {
            return Err(Invalid(format!(
                "physics.stale_steps must be in [1, 1000], got {}",
                self.physics.stale_steps
            )));
        }
        Ok(())
    }
}

/// The aim's windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AimFreshness {
    /// How long the gun aim's snapshot stands for the player. The aim update runs every simulation
    /// tick while the player exists, so an older one belongs to a paused or departed player.
    pub gun_ms: Millis,
}

/// The body's windows: the unarmed pose, the learned estimator, the IK body, the procedural body,
/// and the firm ragdoll.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyFreshness {
    /// How long an unarmed decision holds for the modifier pass, which runs earlier in the next
    /// tick.
    pub unarmed_ms: Millis,
    /// How long after the last frame the eyes stand for the estimator window's position.
    pub estimator_eyes_ms: Millis,
    /// An estimate older than this is stale for any consumer.
    pub estimate_ms: Millis,
    /// A support snapshot older than this no longer stands for its support: the body has gone, or
    /// the probes' update has stopped running.
    pub ik_support_ms: Millis,
    /// The IK body's published FK pose stops standing for the player after this long: a few
    /// rendered frames.
    pub ik_fk_ms: Millis,
    /// How long after the IK body's gate last ran its status still stands: a few ticks.
    pub ik_gate_ms: Millis,
    /// How long after the procedural gate last ran the body still counts as solved: a few ticks.
    pub procedural_gate_ms: Millis,
    /// How long the published local model counts as the player's after it was last refreshed: a
    /// few ticks.
    pub procedural_local_ms: Millis,
    /// How long a published firm ragdoll source stands: a few ticks.
    pub firm_ragdoll_source_ms: Millis,
}

/// The body rig's windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigFreshness {
    /// The grounded rig's published frame stops standing for the rig after this long.
    pub grounded_ms: Millis,
    /// How old the character's pose may be and still place the holsters.
    pub skeleton_ms: Millis,
    /// How long a collision probe result stays current. The probe runs every simulation tick in
    /// gameplay under the headset, so a stale result means the probe has stopped, and its fade must
    /// clear.
    pub probe_ms: Millis,
    /// How old the latest tick's root trace may be for the recorder to sample it.
    pub root_trace_ms: Millis,
    /// A gap between rendered frames longer than this means the headset stopped rendering, and the
    /// root's frame half measurement stops holding for the simulation half.
    pub root_frame_gap_ms: Millis,
}

/// The hands' windows: the anchor, the measured arms, the presentation, and the grab readout.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandsFreshness {
    /// How long the input tick's attach point stays current.
    pub anchor_ms: Millis,
    /// A gap in the hang task's ticks longer than this begins a new hang.
    pub hang_gap_ms: Millis,
    /// How long the hands rig's measured arms stay current after the last measurement.
    pub measured_arms_ms: Millis,
    /// How old the latest tick may be for the presentation layer to pose from it: a few ticks. Past
    /// it the pose pass has stopped running for the player (a cutscene, a load), and the body is
    /// left as drawn.
    pub presentation_ms: Millis,
    /// How long the grab readout counts as live after the last input tick.
    pub grab_status_ms: Millis,
}

/// The finger curls' grip declarations: a declaration lasts a few of the curls' ticks, so that a
/// source ticking at another rate (the input tick, once a frame) is not lost between two of them,
/// and short enough that a hand let go opens at once.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FingersFreshness {
    /// The shortest a declaration lasts, seconds.
    pub grip_min_s: f32,
    /// The longest a declaration lasts, seconds: also how long one counts for the readouts, which do
    /// not know the curls' tick interval.
    pub grip_max_s: f32,
    /// How many of the curls' ticks a declaration lasts, within the two bounds.
    pub grip_ticks: f32,
}

/// The held items' windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldFreshness {
    /// How long a physics step's held-gun results stay current for a reader between the steps. A
    /// publication's staleness counts steps instead ([`PhysicsFreshness::stale_steps`]), so a hitch
    /// of the whole simulation tick never drops a held gun.
    pub guns_ms: Millis,
}

/// Locomotion's windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocomotionFreshness {
    /// Ticks of the movement task further apart than this are separate runs of it.
    pub movement_task_ms: Millis,
    /// How long the strokes may go without a tick before they forget the hands' motion.
    pub strokes_ms: Millis,
}

/// Roomscale's windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomscaleFreshness {
    /// A gap between rendered frames longer than this means the headset stopped rendering: the fade
    /// resets, and the frame half's values stop holding for the simulation half.
    pub frame_gap_ms: Millis,
}

/// The published facts' windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactsFreshness {
    /// A driving view target older than this is ignored: the input tick stopped publishing, so the
    /// view eases back.
    pub driving_ms: Millis,
}

/// The detours' windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HooksFreshness {
    /// How long a layer mask publication holds. The pose-finalization pass republishes every tick.
    pub layer_mask_ms: Millis,
    /// A gap between the body IK animation's ticks longer than this restarts its filter.
    pub body_ik_animation_gap_ms: Millis,
    /// A gap between the parachute's yaw alignment calls longer than this opens a new alignment.
    pub parachute_align_gap_ms: Millis,
}

/// The physics steps' window.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsFreshness {
    /// How many physics steps a publication stays current for the step and for what decides whether
    /// a hand acts. Counted in steps rather than wall-clock time, so a hitch that holds up the whole
    /// simulation tick (a frame of a few hundred milliseconds) ages nothing, and a live hold is not
    /// torn down; only steps that run without a new publication do.
    pub stale_steps: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(FRESHNESS.get().validate(), Ok(()));
    }
}
