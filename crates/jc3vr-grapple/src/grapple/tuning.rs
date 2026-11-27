//! The grapple's tuning, read from the shipped `tuning/grapple.toml` ([`GRAPPLE`]): the comfort
//! filter on the head pose, the free hand's reach for the line, the least held length of any line,
//! the seat grapple, the lock, the physical grapple's pull, reel, tether retract, and hang swing,
//! the grapple button's gestures, and the yank.

use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range, vec3},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::{
    controllers::yank::YankConfig,
    grapple::{
        line_grab::LineReach,
        physical::config::{LockTuning, PhysicalGrappleTuning},
        rope::RopeFloorTuning,
    },
};

jc3vr_core::tuning_file! {
    /// The grapple's tuning file.
    pub static GRAPPLE: GrappleTuning = "grapple.toml";
}

/// The grapple's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrappleTuning {
    pub comfort: ComfortTuning,
    pub line: LineReach,
    pub(crate) rope: RopeFloorTuning,
    pub seat: SeatTuning,
    pub lock: LockTuning,
    pub physical: PhysicalGrappleTuning,
    pub button: GrappleButtonTuning,
    pub yank: YankTuning,
    /// The grapple yank's detector: a sharp pull of the grapple hand toward the body zips along an
    /// attached line. Acts only under the controller scheme.
    pub grapple_yank: YankConfig,
}

/// The comfort filter on the head pose while the grapple owns the character
/// ([`crate::grapple`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComfortTuning {
    /// The blend-out time constant when the grapple ends, seconds, easing the body rotation
    /// accumulated over the reel back into the view instead of snapping to it. Under the yaw
    /// handoff this carries only the small pitch and roll residual.
    pub release_s: f32,
    /// How long the post-reel yaw handoff may hold the view while the body turns toward it,
    /// seconds, before falling back to the blend release (the body can be blocked from turning).
    pub(crate) handoff_timeout_s: f32,
    /// The heading convergence below which the post-reel yaw handoff completes, degrees.
    pub(crate) handoff_converged_deg: f32,
    /// How long a pending yaw-handoff heading stays valid awaiting an on-foot tick, seconds.
    pub(crate) retarget_fresh_s: f32,
    /// The change in one tick of the head anchor's offset from the root beyond which the change
    /// is treated as a landing or attach snap and absorbed, metres. The snap moves the head half a
    /// metre to a metre and a half against the root in one tick; a clip's own motion moves it a
    /// few centimetres a tick, and a canopy's opening up to about a quarter of a metre. The HMD's
    /// own tracking is unaffected.
    pub(crate) anchor_snap_m: f32,
    /// How long an absorbed landing snap takes to ease back out of the view, seconds.
    pub(crate) anchor_snap_ease_s: f32,
    /// Steps of the anchor's offset from the root beyond this are no animation, metres: passed
    /// through, with the absorbed offset reset.
    pub(crate) anchor_teleport_m: f32,
    /// The absorbed offset never exceeds this, metres.
    pub(crate) anchor_offset_max_m: f32,
}

/// The seat grapple ([`crate::grapple::seat`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatTuning {
    /// How long the hook may fly from the seat before it is called back, game seconds. The hook
    /// flies the grapple's range in well under a second.
    pub(crate) flight_s: f32,
    /// How far past the seated aim's point the target's ray reaches, metres.
    pub(crate) ray_past_m: f32,
    /// How near the seated aim's point the ray's hit must be to stand for it, metres.
    pub(crate) hit_within_m: f32,
}

/// The grapple button's gestures ([`crate::controllers::grapple_button`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrappleButtonTuning {
    /// How long a press may be held and still be a tap, seconds.
    pub tap_s: f32,
    /// How soon a second tap must follow a first for a double tap, seconds.
    pub(crate) double_tap_s: f32,
    /// The retract a tightening hold starts at, `0..1`.
    pub(crate) tighten_start: f32,
    /// The retract a tightening hold rises to on its own, `0..1`; the hand's pull tightens past
    /// it.
    pub(crate) tighten_held: f32,
    /// How long a tightening hold takes to rise to [`tighten_held`](Self::tighten_held), seconds.
    pub(crate) tighten_ramp_s: f32,
}

/// What every yank detector shares ([`crate::controllers::yank`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YankTuning {
    /// The most a yank's pull may carry the hand out from the head across the ground, as a share
    /// of the pull. A pull back along the line, down, or back and out past the side keeps the hand
    /// at about its distance from the head across the ground or brings it closer; an arm swung out
    /// to the side takes it well away.
    pub(crate) outward_share: f32,
}

impl Tuning for GrappleTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let comfort = &self.comfort;
        nonnegative("comfort.release_s", comfort.release_s, 10.0)?;
        nonnegative("comfort.handoff_timeout_s", comfort.handoff_timeout_s, 10.0)?;
        nonnegative(
            "comfort.handoff_converged_deg",
            comfort.handoff_converged_deg,
            180.0,
        )?;
        nonnegative("comfort.retarget_fresh_s", comfort.retarget_fresh_s, 10.0)?;
        positive(
            "comfort.anchor_teleport_m",
            comfort.anchor_teleport_m,
            100.0,
        )?;
        range(
            "comfort.anchor_snap_m",
            comfort.anchor_snap_m,
            0.0,
            comfort.anchor_teleport_m,
        )?;
        positive(
            "comfort.anchor_snap_ease_s",
            comfort.anchor_snap_ease_s,
            10.0,
        )?;
        nonnegative(
            "comfort.anchor_offset_max_m",
            comfort.anchor_offset_max_m,
            10.0,
        )?;

        let line = &self.line;
        positive("line.max_ahead_m", line.max_ahead_m, 10.0)?;
        range("line.min_ahead_m", line.min_ahead_m, 0.0, line.max_ahead_m)?;
        nonnegative("line.far_margin_m", line.far_margin_m, 10.0)?;
        positive("line.take_m", line.take_m, 1.0)?;
        vec3("line.palm_from_wrist_m", line.palm_from_wrist_m, 0.5)?;

        positive("rope.min_held_length_m", self.rope.min_held_length_m, 10.0)?;

        let seat = &self.seat;
        positive("seat.flight_s", seat.flight_s, 60.0)?;
        nonnegative("seat.ray_past_m", seat.ray_past_m, 10.0)?;
        nonnegative("seat.hit_within_m", seat.hit_within_m, 10.0)?;

        let button = &self.button;
        positive("button.tap_s", button.tap_s, 10.0)?;
        positive("button.double_tap_s", button.double_tap_s, 10.0)?;
        range("button.tighten_start", button.tighten_start, 0.0, 1.0)?;
        range(
            "button.tighten_held",
            button.tighten_held,
            button.tighten_start,
            1.0,
        )?;
        positive("button.tighten_ramp_s", button.tighten_ramp_s, 60.0)?;

        range("yank.outward_share", self.yank.outward_share, 0.0, 10.0)?;
        self.grapple_yank.validate("grapple_yank")?;

        self.lock.validate()?;
        self.physical.validate()
    }
}

impl LockTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("lock.stiffness_per_s2", self.stiffness_per_s2, 10_000.0)?;
        nonnegative("lock.damping_per_s", self.damping_per_s, 1_000.0)?;
        positive("lock.max_accel_g", self.max_accel_g, 100.0)?;
        nonnegative("lock.break_stretch_m", self.break_stretch_m, 100.0)?;
        nonnegative("lock.reel_in_mps", self.reel_in_mps, 100.0)?;
        nonnegative("lock.min_length_m", self.min_length_m, 100.0)?;
        range("lock.reaction", self.reaction, 0.0, 1.0)?;
        positive("lock.hang_length_m", self.hang_length_m, 100.0)?;
        nonnegative("lock.coast_brake_g", self.coast_brake_g, 100.0)?;
        range(
            "lock.coast_strength_share",
            self.coast_strength_share,
            0.0,
            1.0,
        )?;
        nonnegative("lock.pay_out_take_in_m", self.pay_out_take_in_m, 10.0)?;
        nonnegative("lock.pay_out_mps", self.pay_out_mps, 100.0)?;
        positive(
            "lock.hang_swing_damping_s",
            self.hang_swing_damping_s,
            100.0,
        )?;
        nonnegative("lock.moving_hook_mps", self.moving_hook_mps, 100.0)?;
        nonnegative("lock.handback_for_s", self.handback_for_s, 10.0)?;
        range(
            "lock.handback_settle_s",
            self.handback_settle_s,
            0.0,
            self.handback_for_s,
        )?;
        range("lock.handback_carried", self.handback_carried, 0.0, 1.0)?;
        milliseconds("lock.entering_ms", self.entering_ms)?;
        milliseconds("lock.hang_wanted_ms", self.hang_wanted_ms)?;
        milliseconds("lock.hang_refused_ms", self.hang_refused_ms)?;
        range("lock.level_min", self.level_min, 0.0, 1.0)?;

        let pump = &self.pump;
        nonnegative("lock.pump.accel_g", pump.accel_g, 10.0)?;
        positive("lock.pump.taut_m", pump.taut_m, 10.0)?;
        positive("lock.pump.max_step_m", pump.max_step_m, 10.0)?;
        positive("lock.pump.max_speed_mps", pump.max_speed_mps, 100.0)?;
        milliseconds("lock.pump.stale_ms", pump.stale_ms)?;
        milliseconds("lock.pump.frame_stale_ms", pump.frame_stale_ms)
    }
}

impl PhysicalGrappleTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let pull = &self.pull;
        nonnegative("physical.pull.speed_hold_s", pull.speed_hold_s, 10.0)?;
        nonnegative("physical.pull.relax_mps", pull.relax_mps, 10.0)?;

        let reel = &self.reel;
        positive("physical.reel.min_scale", reel.min_scale, 10.0)?;
        range(
            "physical.reel.max_scale",
            reel.max_scale,
            reel.min_scale,
            10.0,
        )?;
        positive("physical.reel.full_speed_mps", reel.full_speed_mps, 100.0)?;
        nonnegative("physical.reel.boost_s", reel.boost_s, 10.0)?;
        milliseconds("physical.reel.drive_max_age_ms", reel.drive_max_age_ms)?;

        let tether = &self.tether;
        positive("physical.tether.full_m", tether.full_m, 10.0)?;
        range(
            "physical.tether.deadzone_m",
            tether.deadzone_m,
            0.0,
            tether.full_m,
        )?;

        let swing = &self.swing;
        positive("physical.swing.full_mps", swing.full_mps, 100.0)?;
        range(
            "physical.swing.deadzone_mps",
            swing.deadzone_mps,
            0.0,
            swing.full_mps,
        )?;
        range("physical.swing.scale", swing.scale, -10.0, 10.0)?;
        nonnegative("physical.swing.max_scale", swing.max_scale, 10.0)?;
        nonnegative("physical.swing.smoothing_s", swing.smoothing_s, 10.0)?;

        milliseconds("physical.status_max_age_ms", self.status_max_age_ms)
    }
}

/// Check that a window of `value` milliseconds is at most a minute.
fn milliseconds(name: &str, value: u64) -> Result<(), Invalid> {
    nonnegative(name, value as f32, 60_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(GRAPPLE.get().validate(), Ok(()));
    }
}
