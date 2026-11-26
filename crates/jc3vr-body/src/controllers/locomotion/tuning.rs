//! The locomotion's tuning, read from the shipped `tuning/locomotion.toml` ([`LOCOMOTION`]): the
//! stick's integrator and its stillness thresholds, the act filter's watchdog, the stable view
//! anchor, its height smoothing, the held view's head follow, and the arm strokes.

use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::{
    controllers::locomotion::{accel::Rates, strokes::model::StrokeTuning},
    tuning::Millis,
};

jc3vr_core::tuning_file! {
    /// The locomotion's tuning file.
    pub static LOCOMOTION: LocomotionTuning = "locomotion.toml";
}

/// The locomotion tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocomotionTuning {
    pub stick: StickTuning,
    pub acts: ActsTuning,
    pub anchor: AnchorTuning,
    pub height: HeightTuning,
    pub follow: FollowTuning,
    pub strokes: StrokeTuning,
}

impl Tuning for LocomotionTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let s = &self.stick;
        positive(
            "stick.rates.accelerate_mps2",
            s.rates.accelerate_mps2,
            100.0,
        )?;
        positive(
            "stick.rates.decelerate_mps2",
            s.rates.decelerate_mps2,
            100.0,
        )?;
        range("stick.direction_min", s.direction_min, 0.0, 1.0)?;
        range("stick.still_stick", s.still_stick, 0.0, 1.0)?;
        nonnegative("stick.still_speed_mps", s.still_speed_mps, 5.0)?;
        range("stick.min_forward", s.min_forward, 0.0, 1.0)?;

        let a = &self.acts;
        positive("acts.watchdog_s", a.watchdog_s, 60.0)?;
        positive("acts.release_s", a.release_s, 60.0)?;
        a.burst_gap_ms.validate("acts.burst_gap_ms", 10_000)?;

        let a = &self.anchor;
        nonnegative("anchor.face_clearance_m", a.face_clearance_m, 0.5)?;
        positive("anchor.capture_s", a.capture_s, 60.0)?;
        nonnegative("anchor.blend_s", a.blend_s, 10.0)?;
        nonnegative("anchor.jump_grace_s", a.jump_grace_s, 10.0)?;

        let h = &self.height;
        positive("height.release_s", h.release_s, 10.0)?;
        nonnegative("height.smoothing_s", h.smoothing_s, 10.0)?;
        nonnegative("height.max_lag_m", h.max_lag_m, 5.0)?;
        range("height.snap_m", h.snap_m, h.max_lag_m, 10.0)?;

        let f = &self.follow;
        nonnegative("follow.smoothing_s", f.smoothing_s, 10.0)?;
        positive("follow.max_speed_mps", f.max_speed_mps, 100.0)?;
        nonnegative("follow.return_smoothing_s", f.return_smoothing_s, 10.0)?;
        positive("follow.escape_m", f.escape_m, 10.0)?;
        positive("follow.escape_drop_m", f.escape_drop_m, 10.0)?;
        nonnegative("follow.escape_s", f.escape_s, 10.0)?;
        range("follow.escape_return_m", f.escape_return_m, 0.0, f.escape_m)?;

        self.strokes.validate()
    }
}

/// The stick locomotion's integrator and what counts as still.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StickTuning {
    /// The integrator's fixed rates. The deceleration is higher than the acceleration, so stopping
    /// is crisp.
    pub rates: Rates,
    /// A stick shorter than this has no direction, `0..1`.
    pub direction_min: f32,
    /// A move stick shorter than this is no stick, for standing still and for the player's own
    /// stick under a committed step, `0..1`.
    pub still_stick: f32,
    /// Below this the task's and the stick's velocities are no motion, metres per second.
    pub still_speed_mps: f32,
    /// The shortest horizontal head forward that still has a heading, `0..1`; a head looking
    /// further up or down takes Rico's own forward as the stick's frame.
    pub min_forward: f32,
}

/// The act filter's watchdog ([`crate::controllers::locomotion::acts::Watchdog`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActsTuning {
    /// How long the filter may act without a break before the watchdog decides the animation state
    /// machine is waiting on an act the filter keeps from it, seconds.
    pub watchdog_s: f32,
    /// How long the watchdog then lets every act through, seconds.
    pub release_s: f32,
    /// Acts closer together than this belong to one burst.
    pub burst_gap_ms: Millis,
}

/// The stable view anchor ([`crate::controllers::locomotion::anchor`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorTuning {
    /// How far in front of the animated face the eyes are held while a clip poses the body,
    /// metres: a few centimetres past the skull's front, so a chest or a shoulder the pose folds
    /// toward the face stays beyond the near plane.
    pub face_clearance_m: f32,
    /// How long Rico must have stood still for his neutral head to be measured, seconds.
    pub capture_s: f32,
    /// How long the anchor takes to hand over between the animated head and the root, seconds.
    pub blend_s: f32,
    /// How long after leaving the ground the anchor stays on the root, seconds, so a jump keeps the
    /// view steady while a long fall hands it back to the animation. The procedural body's
    /// hand-over table reads a jump by the same grace.
    pub jump_grace_s: f32,
}

/// The smoothing of the root's height under the stable anchor
/// ([`crate::controllers::locomotion::view::HeightFilter`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeightTuning {
    /// How long the offset the smoothing hid takes to wind down once the body leaves the ground,
    /// seconds.
    pub release_s: f32,
    /// The time constant of the smoothing of the root's height while grounded, seconds. Turns a
    /// staircase's steps into a ramp.
    pub smoothing_s: f32,
    /// The furthest the smoothed height may trail the root's, metres.
    pub max_lag_m: f32,
    /// A height change beyond this in one tick is a teleport or a drop, and is taken at once,
    /// metres.
    pub snap_m: f32,
}

/// The held view's head follow and its escape
/// ([`crate::controllers::locomotion::view_follow`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowTuning {
    /// The smoothing of the followed head, seconds: it settles in about a third of a second, so a
    /// get-up's rise is followed and a ragdoll's flail is eased.
    pub smoothing_s: f32,
    /// The fastest the followed head moves about the root, metres per second: a ragdoll's first
    /// fall eases in rather than dropping the view.
    pub max_speed_mps: f32,
    /// The smoothing of the difference left on the way back to the root, seconds.
    pub return_smoothing_s: f32,
    /// How far the animated head must be from the root's, horizontally, to count toward the
    /// escape, metres: past a walk's lean and a crouch's forward reach.
    pub escape_m: f32,
    /// How far the animated head must drop below the root's to count toward the escape as
    /// [`escape_m`](Self::escape_m) does horizontally, metres: below a landing's crouch, above a
    /// body lying down.
    pub escape_drop_m: f32,
    /// How long the head must stay that far for the view to follow it, seconds: longer than a
    /// landing's dip.
    pub escape_s: f32,
    /// How close the head must come back for the view to return to the root, metres.
    pub escape_return_m: f32,
}

impl StrokeTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("strokes.speed_per_metre", self.speed_per_metre, 100.0)?;
        positive("strokes.decay_s", self.decay_s, 60.0)?;
        nonnegative("strokes.max_speed_mps", self.max_speed_mps, 50.0)?;
        nonnegative("strokes.min_speed_mps", self.min_speed_mps, 10.0)?;
        range("strokes.palm_cone_deg", self.palm_cone_deg, 0.0, 89.0)?;
        positive("strokes.catch_mps", self.catch_mps, 10.0)?;
        range("strokes.turn_gain", self.turn_gain, -100.0, 100.0)?;
        positive("strokes.turn_smoothing_s", self.turn_smoothing_s, 10.0)?;
        nonnegative("strokes.max_turn_deg_s", self.max_turn_deg_s, 720.0)?;
        nonnegative("strokes.water_below_eyes_m", self.water_below_eyes_m, 2.0)?;
        positive(
            "strokes.velocity_smoothing_s",
            self.velocity_smoothing_s,
            1.0,
        )?;
        nonnegative("strokes.catch_reach_m", self.catch_reach_m, 2.0)?;
        nonnegative(
            "strokes.finish_behind_head_m",
            self.finish_behind_head_m,
            2.0,
        )?;
        positive("strokes.catch_travel_m", self.catch_travel_m, 2.0)?;
        range(
            "strokes.breaststroke_sweep_share",
            self.breaststroke_sweep_share,
            0.0,
            1.0,
        )?;
        positive("strokes.steer_travel_m", self.steer_travel_m, 10.0)?;
        range("strokes.palm_against_cos", self.palm_against_cos, -1.0, 1.0)?;
        range("strokes.min_alignment", self.min_alignment, 0.0, 1.0)?;
        range(
            "strokes.catch_min_strength",
            self.catch_min_strength,
            0.0,
            1.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(LOCOMOTION.get().validate(), Ok(()));
    }
}
