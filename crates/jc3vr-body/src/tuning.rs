//! The body crate's shared tuning: the [`Millis`] form its files write a wall-clock duration in, and
//! the freshness windows ([`FRESHNESS`]), which say how long each publication the body's subsystems
//! read from one another stays current. The windows are gathered in one file, rather than beside
//! each subsystem's other tuning, so that every staleness rule the crate applies is listed in one
//! place.

use std::time::Duration;

use jc3vr_core::{
    config::{Invalid, positive, range},
    published::Window,
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

    /// The duration as a freshness window on the wall clock.
    pub const fn window(self) -> Window {
        Window::from_millis(self.0)
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
    pub overlay: OverlayFreshness,
    pub facts: FactsFreshness,
    pub hooks: HooksFreshness,
    pub physics: PhysicsFreshness,
}

impl Tuning for FreshnessTuning {
    fn validate(&self) -> Result<(), Invalid> {
        const MAX_MS: u64 = 10_000;
        let windows = [
            ("aim.gun_ms", self.aim.gun_ms),
            ("aim.grapple_weapons_ms", self.aim.grapple_weapons_ms),
            ("aim.sight_request_ms", self.aim.sight_request_ms),
            ("body.mirror_ms", self.body.mirror_ms),
            ("body.ik_support_kept_ms", self.body.ik_support_kept_ms),
            ("body.ik_probe_answer_ms", self.body.ik_probe_answer_ms),
            ("body.ik_brace_answer_ms", self.body.ik_brace_answer_ms),
            (
                "body.procedural_climb_task_ms",
                self.body.procedural_climb_task_ms,
            ),
            ("body.wings_gap_ms", self.body.wings_gap_ms),
            ("rig.snap_wait_ms", self.rig.snap_wait_ms),
            ("hands.grab_markers_ms", self.hands.grab_markers_ms),
            ("overlay.sim_ms", self.overlay.sim_ms),
            ("overlay.frame_ms", self.overlay.frame_ms),
            ("body.unarmed_ms", self.body.unarmed_ms),
            ("body.estimator_eyes_ms", self.body.estimator_eyes_ms),
            ("body.estimate_ms", self.body.estimate_ms),
            ("body.ik_support_ms", self.body.ik_support_ms),
            ("body.ik_fk_ms", self.body.ik_fk_ms),
            ("body.gate_ms", self.body.gate_ms),
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
        positive(
            "hands.elbow_smoothing_gap_s",
            self.hands.elbow_smoothing_gap_s,
            10.0,
        )?;
        positive(
            "locomotion.movement_task_grace_s",
            self.locomotion.movement_task_grace_s,
            10.0,
        )?;
        positive(
            "locomotion.strokes_max_tick_s",
            self.locomotion.strokes_max_tick_s,
            10.0,
        )?;
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
    pub(crate) gun_ms: Millis,
    /// How long a published grapple-weapon aim stays current.
    pub(crate) grapple_weapons_ms: Millis,
    /// How long a sight tool request waits for an aim update before it is dropped.
    pub(crate) sight_request_ms: Millis,
}

/// The body's windows: the unarmed pose, the learned estimator, the IK body, the procedural body,
/// and the firm ragdoll.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyFreshness {
    /// How long an unarmed decision holds for the modifier pass, which runs earlier in the next
    /// tick.
    pub(crate) unarmed_ms: Millis,
    /// How long after the last frame the eyes stand for the estimator window's position.
    pub(crate) estimator_eyes_ms: Millis,
    /// An estimate older than this is stale for any consumer.
    pub(crate) estimate_ms: Millis,
    /// A support snapshot older than this no longer stands for its support: the body has gone, or
    /// the probes' update has stopped running.
    pub(crate) ik_support_ms: Millis,
    /// The IK body's published FK pose stops standing for the player after this long: a few
    /// rendered frames.
    pub(crate) ik_fk_ms: Millis,
    /// How long after the procedural body's solve gate last ran its status, and the IK body's
    /// status written in the same gate, still stand: a few ticks.
    pub(crate) gate_ms: Millis,
    /// How long the published local model counts as the player's after it was last refreshed: a
    /// few ticks.
    pub(crate) procedural_local_ms: Millis,
    /// How long a published firm ragdoll source stands: a few ticks.
    pub(crate) firm_ragdoll_source_ms: Millis,
    /// How long a body mirror submission is drawn without another: a few ticks, so a paused or
    /// skipped pass clears the mirror.
    pub(crate) mirror_ms: Millis,
    /// A support the IK body's gate asks about stays tracked this long after it last asked.
    pub(crate) ik_support_kept_ms: Millis,
    /// A ground probe's answer stands for its point this long.
    pub(crate) ik_probe_answer_ms: Millis,
    /// A brace cast's answer stands this long: a few ticks, so a brace taken from it is near where
    /// the leg is.
    pub(crate) ik_brace_answer_ms: Millis,
    /// How long after the climb's task last ran it still counts as running: a tick or two.
    pub(crate) procedural_climb_task_ms: Millis,
    /// A gap between the wings' readings longer than this starts their follower again: a few
    /// ticks.
    pub(crate) wings_gap_ms: Millis,
}

/// The body rig's windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigFreshness {
    /// The grounded rig's published frame stops standing for the rig after this long.
    pub(crate) grounded_ms: Millis,
    /// How old the character's pose may be and still place the holsters.
    pub(crate) skeleton_ms: Millis,
    /// How long a collision probe result stays current. The probe runs every simulation tick in
    /// gameplay under the headset, so a stale result means the probe has stopped, and its fade must
    /// clear.
    pub(crate) probe_ms: Millis,
    /// How old the latest tick's root trace may be for the recorder to sample it.
    pub(crate) root_trace_ms: Millis,
    /// A gap between rendered frames longer than this means the headset stopped rendering, and the
    /// root's frame half measurement stops holding for the simulation half.
    pub(crate) root_frame_gap_ms: Millis,
    /// How long a recenter's snap waits for Rico to be grounded before it expires.
    pub(crate) snap_wait_ms: Millis,
}

/// The hands' windows: the anchor, the measured arms, the presentation, and the grab readout.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandsFreshness {
    /// How long the input tick's attach point stays current.
    pub(crate) anchor_ms: Millis,
    /// A gap in the hang task's ticks longer than this begins a new hang.
    pub(crate) hang_gap_ms: Millis,
    /// How long the hands rig's measured arms stay current after the last measurement.
    pub(crate) measured_arms_ms: Millis,
    /// How old the latest tick may be for the presentation layer to pose from it: a few ticks. Past
    /// it the pose pass has stopped running for the player (a cutscene, a load), and the body is
    /// left as drawn.
    pub(crate) presentation_ms: Millis,
    /// How long the grab readout counts as live after the last input tick.
    pub(crate) grab_status_ms: Millis,
    /// How long the grab markers live in the overlay. They are submitted at the input tick's rate,
    /// so they live a little past one tick.
    pub(crate) grab_markers_ms: Millis,
    /// The longest step the elbow pole's smoothing carries its direction across, seconds; past it
    /// (the layer was idle, a hitch) the smoothing starts again from the rule's pole.
    pub(crate) elbow_smoothing_gap_s: f32,
}

/// The finger curls' grip declarations: a declaration lasts a few of the curls' ticks, so that a
/// source ticking at another rate (the input tick, once a frame) is not lost between two of them,
/// and short enough that a hand let go opens at once.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FingersFreshness {
    /// The shortest a declaration lasts, seconds.
    pub(crate) grip_min_s: f32,
    /// The longest a declaration lasts, seconds: also how long one counts for the readouts, which do
    /// not know the curls' tick interval.
    pub(crate) grip_max_s: f32,
    /// How many of the curls' ticks a declaration lasts, within the two bounds.
    pub(crate) grip_ticks: f32,
}

/// The held items' windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldFreshness {
    /// How long a physics step's held-gun results stay current for a reader between the steps. A
    /// publication's staleness counts steps instead ([`PhysicsFreshness::stale_steps`]), so a hitch
    /// of the whole simulation tick never drops a held gun.
    pub(crate) guns_ms: Millis,
}

/// Locomotion's windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocomotionFreshness {
    /// Ticks of the movement task further apart than this are separate runs of it.
    pub(crate) movement_task_ms: Millis,
    /// How long the strokes may go without a tick before they forget the hands' motion.
    pub(crate) strokes_ms: Millis,
    /// How long after the movement task last ran it still counts as driving the root, seconds. It
    /// runs every simulation tick on foot, so this covers a few ticks' jitter without letting an
    /// unclassified animation carry the body far from the view first.
    pub(crate) movement_task_grace_s: f32,
    /// The longest tick that can contain a stroke, seconds. Longer gaps forget the hands' motion.
    pub(crate) strokes_max_tick_s: f32,
}

/// Roomscale's windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomscaleFreshness {
    /// A gap between rendered frames longer than this means the headset stopped rendering: the fade
    /// resets, and the frame half's values stop holding for the simulation half.
    pub(crate) frame_gap_ms: Millis,
}

/// The overlay producers' windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayFreshness {
    /// The time to live of the simulation-rate producers' slots: a few simulation ticks.
    pub(crate) sim_ms: Millis,
    /// The time to live of the frame-rate producers' slots.
    pub frame_ms: Millis,
}

/// The published facts' windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactsFreshness {
    /// A driving view target older than this is ignored: the input tick stopped publishing, so the
    /// view eases back.
    pub(crate) driving_ms: Millis,
}

/// The detours' windows.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HooksFreshness {
    /// How long a layer mask publication holds. The pose-finalization pass republishes every tick.
    pub(crate) layer_mask_ms: Millis,
    /// A gap between the body IK animation's ticks longer than this restarts its filter.
    pub(crate) body_ik_animation_gap_ms: Millis,
    /// A gap between the parachute's yaw alignment calls longer than this opens a new alignment.
    pub(crate) parachute_align_gap_ms: Millis,
}

/// The physics steps' window.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsFreshness {
    /// How many physics steps a publication stays current for the step and for what decides whether
    /// a hand acts. Counted in steps rather than wall-clock time, so a hitch that holds up the whole
    /// simulation tick (a frame of a few hundred milliseconds) ages nothing, and a live hold is not
    /// torn down; only steps that run without a new publication do.
    pub(crate) stale_steps: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(FRESHNESS.get().validate(), Ok(()));
    }
}
