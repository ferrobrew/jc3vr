//! The airborne tuning: the wingsuit's arm steering, gestures, evade, and take-off, the
//! parachute's handles and rig, and the ripcord.

use std::time::Duration;

use glam::Vec3;
use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range, vec3},
    published::Window,
};
use serde::Deserialize;

use crate::vehicles::{parachute::ripcord::MIN_PULL_M, tuning::millis};

/// The arm steering's thresholds (see [`crate::vehicles::wingsuit::arm_steering`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmSteeringTuning {
    /// Height differences and offsets below this steer nothing, metres. The bank, and the height
    /// and sweep pitches, share it.
    pub deadzone_m: f32,
    /// The height difference between the hands that banks fully, metres.
    pub(crate) full_bank_m: f32,
    /// How far apart the hands must be, as a share of the arm span, for the arms to count as
    /// spread. Closer together, the arms steer nothing and the stick steers alone.
    pub(crate) min_spread: f32,
    /// How much narrower than the engaging spread the arms may come in, as a share of the arm
    /// span, before they stop steering.
    pub(crate) spread_hysteresis: f32,
    /// How far both hands must be above or below the shoulders to pitch fully, metres, under
    /// [`crate::vehicles::config::ArmPitch::Height`].
    pub(crate) full_pitch_m: f32,
    /// How far the wrists must tilt from the neutral to pitch fully, degrees, under
    /// [`crate::vehicles::config::ArmPitch::WristTilt`].
    pub(crate) full_tilt_deg: f32,
    /// Wrist tilts within this of the neutral pitch nothing, degrees.
    pub(crate) tilt_deadzone_deg: f32,
    /// How far both hands must be swept ahead of or behind the shoulders to pitch fully, metres,
    /// under [`crate::vehicles::config::ArmPitch::Sweep`].
    pub(crate) full_sweep_m: f32,
}

/// The bilateral wingsuit gestures: fresh-pose deployment, bilateral withdrawal, and palms-forward
/// braking (see [`crate::vehicles::wingsuit::gestures`]). Distances are shares of the
/// calibrated arm span.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WingsuitGesturesTuning {
    /// Each wrist's minimum outward distance from its own shoulder for a flight pose.
    pub flight_lateral_span: f32,
    /// Maximum arm angle away from the outward horizontal direction for a fresh flight pose.
    pub flight_angle_deg: f32,
    /// Each wrist must be this close laterally before withdrawal qualifies.
    pub withdraw_lateral_span: f32,
    /// Each wrist qualifies as withdrawn when lowered this far below its shoulder.
    pub withdraw_lowered_span: f32,
    /// A tucked wrist also qualifies within this shoulder distance, without lowering the arm.
    pub withdraw_reach_span: f32,
    /// How far inside the flight pose's spread a held brake's arms may come before it releases.
    pub pose_hysteresis_span: f32,
    /// Each wrist's largest roll from palm-flat, either way, for a flight pose. Both palms back
    /// within it release a held brake.
    pub flight_roll_deg: f32,
    /// Each wrist's smallest roll toward palm-forward to enter braking.
    pub brake_roll_deg: f32,
    /// Each wrist's largest roll toward palm-forward to enter braking, short of palm-up. A held
    /// brake ignores it.
    pub brake_roll_max_deg: f32,
    pub neutral_dwell_s: f32,
    pub flight_dwell_s: f32,
    pub close_dwell_s: f32,
    pub brake_dwell_s: f32,
    /// A larger pose-time gap invalidates all pending gestures.
    pub max_sample_gap_s: f32,
    /// How long after a deploy request another may follow in the same fall, seconds. The suit's
    /// entry reaches the mode detection within a few ticks, so a fall still running this long
    /// after a request is one the game refused it in (still rising from a jump, or too near the
    /// ground), and a repeat never lands in the suit, where the same action closes it.
    pub deploy_retry_s: f32,
    /// How long a deploy request stays wanted before it lapses, seconds.
    pub deploy_intent_s: f32,
    /// The wrists' smallest horizontal separation that gives the body's facing, metres. Closer,
    /// the line between them turns with any small movement of either hand.
    pub min_arms_separation_m: f32,
    /// The largest angle between the arms' facing and the head's heading that is still read as
    /// the body's facing, degrees. A head turns about as far on the shoulders; past it the arms
    /// are crossed or reaching behind the player, and their line says nothing about the body.
    pub max_arms_from_head_deg: f32,
}

/// The boost wingsuit's barrel-roll evade's throw (see
/// [`crate::vehicles::wingsuit::evade`]). The bank is the angle of the line between
/// the hands from level, positive with the left hand above the right.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WingsuitEvadeTuning {
    /// The bank, either way, a throw must reach, degrees.
    pub(crate) throw_bank_deg: f32,
    /// How far the bank must swing toward the throw's side within [`Self::throw_window_s`],
    /// degrees.
    pub(crate) throw_sweep_deg: f32,
    /// The time the swing must happen in, seconds.
    pub(crate) throw_window_s: f32,
    /// The bank, either way, the arms must come back within before another throw counts, degrees.
    pub(crate) rearm_bank_deg: f32,
    /// The time after a throw before another is taken, seconds.
    pub cooldown_s: f32,
    /// The arms' pitch, as a share of a full pitch, that turns the throw's side roll into its
    /// diagonal: below it the evade rolls straight to the side.
    pub(crate) diagonal_pitch: f32,
    /// How long a throw's direction is written over the move stick, seconds: long enough for the
    /// animation graph to pick the roll from it on the ticks after the click.
    pub(crate) direction_hold_s: f32,
    /// The hands' smallest separation, as a share of the arm span, at which their height
    /// difference reads as a bank: the arms spread as wings, not drawn in.
    pub(crate) min_separation: f32,
}

/// How the parachute's handles pull (see [`crate::vehicles::parachute::handles`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParachuteHandlesTuning {
    /// How far down from where it was taken a handle is pulled to act fully, metres.
    pub(crate) full_pull_m: f32,
    /// How far both handles must be pulled, as a share of a full pull, before they brake. Below it,
    /// both pulled together only bank by their difference.
    pub(crate) brake_threshold: f32,
    /// How far from a handle's centre a hand's grip may be for a grip press to take it, metres.
    /// Within it, the handle and any other target in reach go to the nearer, so a press right at a
    /// shoulder socket takes the socket.
    pub radius_m: f32,
    /// How far from a handle's centre a grip press outside every other volume still takes it
    /// rather than firing the grapple, metres. At least [`Self::radius_m`]. It reaches past the
    /// other handle's volume at the usual spacing, which only that handle's own hand can take.
    pub reach_m: f32,
    /// Pulls shorter than this act on nothing, metres.
    pub deadzone_m: f32,
}

/// The parachute's rig (see [`crate::vehicles::parachute::rig`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParachuteRigTuning {
    /// How old the rig's held handles and its native samples may be and still stand,
    /// milliseconds.
    pub stale_after_ms: u64,
}

impl ParachuteRigTuning {
    /// [`Self::stale_after_ms`].
    pub fn stale_after(&self) -> Window {
        Window::from_millis(self.stale_after_ms)
    }

    pub(super) fn validate(&self) -> Result<(), Invalid> {
        millis("parachute_rig.stale_after_ms", self.stale_after_ms, 10_000)
    }
}

/// The wingsuit's take-off from the ground (see [`crate::vehicles::wingsuit::take_off`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TakeOffTuning {
    /// How long the use button must be held on the ground before the body turns, seconds: long
    /// enough that a tap to pick something up or use it does not turn the body, and well short of
    /// the hold that starts the take-off's wind-up, so the body has the rest of the hold to come
    /// around.
    pub(crate) turn_after_s: f32,
    /// The effector value at which the take-off action counts as down, `0..1`: half of a full
    /// press.
    pub(crate) held_value: f32,
}

impl TakeOffTuning {
    /// [`Self::turn_after_s`], to the millisecond.
    pub(crate) fn turn_after(&self) -> Duration {
        Duration::from_millis((self.turn_after_s * 1000.0) as u64)
    }

    pub(super) fn validate(&self) -> Result<(), Invalid> {
        nonnegative("take_off.turn_after_s", self.turn_after_s, 5.0)?;
        range("take_off.held_value", self.held_value, 0.0, 1.0)
    }
}

/// The ripcord (see [`crate::vehicles::parachute::ripcord`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RipcordTuning {
    /// How far the holding hand must come from where it took the ripcord to pull it, metres.
    pub pull_m: f32,
    /// The ripcord's centre from the hip joint along the torso's axes (+X out, +Y up, +Z back),
    /// metres: a little outside, above, and behind the joint, at the back of the hip, behind the
    /// holster.
    pub(crate) from_hip_m: Vec3,
    /// How far from the ripcord's centre a hand's grip may be for a grip press to take it over
    /// every other volume, metres.
    pub radius_m: f32,
    /// How far from the ripcord's centre a grip press outside every other volume still takes it,
    /// metres. At least [`Self::radius_m`].
    pub reach_m: f32,
}

impl RipcordTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        range("ripcord.pull_m", self.pull_m, MIN_PULL_M, 2.0)?;
        vec3("ripcord.from_hip_m", self.from_hip_m, 1.0)?;
        positive("ripcord.radius_m", self.radius_m, 1.0)?;
        range("ripcord.reach_m", self.reach_m, self.radius_m, 2.0)
    }
}

impl ArmSteeringTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        positive("arm_steering.full_bank_m", self.full_bank_m, 2.0)?;
        range(
            "arm_steering.deadzone_m",
            self.deadzone_m,
            0.0,
            self.full_bank_m,
        )?;
        range("arm_steering.min_spread", self.min_spread, 0.0, 2.0)?;
        range(
            "arm_steering.spread_hysteresis",
            self.spread_hysteresis,
            0.0,
            self.min_spread,
        )?;
        for (name, full) in [
            ("full_pitch_m", self.full_pitch_m),
            ("full_sweep_m", self.full_sweep_m),
        ] {
            range(&format!("arm_steering.{name}"), full, self.deadzone_m, 2.0)?;
        }
        positive("arm_steering.full_tilt_deg", self.full_tilt_deg, 180.0)?;
        range(
            "arm_steering.tilt_deadzone_deg",
            self.tilt_deadzone_deg,
            0.0,
            self.full_tilt_deg,
        )
    }
}

impl WingsuitGesturesTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        for (name, span) in [
            ("flight_lateral_span", self.flight_lateral_span),
            ("withdraw_lateral_span", self.withdraw_lateral_span),
            ("withdraw_lowered_span", self.withdraw_lowered_span),
            ("withdraw_reach_span", self.withdraw_reach_span),
            ("pose_hysteresis_span", self.pose_hysteresis_span),
        ] {
            nonnegative(&format!("wingsuit_gestures.{name}"), span, 2.0)?;
        }
        for (name, angle) in [
            ("flight_angle_deg", self.flight_angle_deg),
            ("flight_roll_deg", self.flight_roll_deg),
            ("brake_roll_deg", self.brake_roll_deg),
        ] {
            nonnegative(&format!("wingsuit_gestures.{name}"), angle, 180.0)?;
        }
        range(
            "wingsuit_gestures.brake_roll_max_deg",
            self.brake_roll_max_deg,
            self.brake_roll_deg,
            180.0,
        )?;
        for (name, dwell) in [
            ("neutral_dwell_s", self.neutral_dwell_s),
            ("flight_dwell_s", self.flight_dwell_s),
            ("close_dwell_s", self.close_dwell_s),
            ("brake_dwell_s", self.brake_dwell_s),
        ] {
            nonnegative(&format!("wingsuit_gestures.{name}"), dwell, 5.0)?;
        }
        positive(
            "wingsuit_gestures.max_sample_gap_s",
            self.max_sample_gap_s,
            5.0,
        )?;
        nonnegative(
            "wingsuit_gestures.deploy_retry_s",
            self.deploy_retry_s,
            10.0,
        )?;
        nonnegative(
            "wingsuit_gestures.deploy_intent_s",
            self.deploy_intent_s,
            10.0,
        )?;
        nonnegative(
            "wingsuit_gestures.min_arms_separation_m",
            self.min_arms_separation_m,
            2.0,
        )?;
        range(
            "wingsuit_gestures.max_arms_from_head_deg",
            self.max_arms_from_head_deg,
            0.0,
            180.0,
        )
    }
}

impl WingsuitEvadeTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        positive("wingsuit_evade.throw_bank_deg", self.throw_bank_deg, 180.0)?;
        positive(
            "wingsuit_evade.throw_sweep_deg",
            self.throw_sweep_deg,
            360.0,
        )?;
        positive("wingsuit_evade.throw_window_s", self.throw_window_s, 5.0)?;
        range(
            "wingsuit_evade.rearm_bank_deg",
            self.rearm_bank_deg,
            0.0,
            self.throw_bank_deg,
        )?;
        nonnegative("wingsuit_evade.cooldown_s", self.cooldown_s, 30.0)?;
        range(
            "wingsuit_evade.diagonal_pitch",
            self.diagonal_pitch,
            0.0,
            1.0,
        )?;
        nonnegative(
            "wingsuit_evade.direction_hold_s",
            self.direction_hold_s,
            5.0,
        )?;
        range(
            "wingsuit_evade.min_separation",
            self.min_separation,
            0.0,
            2.0,
        )
    }
}

impl ParachuteHandlesTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        positive("parachute_handles.full_pull_m", self.full_pull_m, 2.0)?;
        range(
            "parachute_handles.brake_threshold",
            self.brake_threshold,
            0.0,
            1.0,
        )?;
        positive("parachute_handles.radius_m", self.radius_m, 1.0)?;
        range(
            "parachute_handles.reach_m",
            self.reach_m,
            self.radius_m,
            2.0,
        )?;
        range(
            "parachute_handles.deadzone_m",
            self.deadzone_m,
            0.0,
            self.full_pull_m,
        )
    }
}
