//! The seats' tuning: the mounted guns and their recoil, the gunner's seat, the driving view, and
//! the cockpit calibration.

use std::time::Duration;

use glam::Vec3;
use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range, vec3},
    published::Window,
};
use serde::Deserialize;

use crate::vehicles::tuning::millis;

/// The mounted guns (see [`crate::vehicles::mounted`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountedTuning {
    /// How old the mounted gun read for the tick may be and still stand, milliseconds.
    pub(crate) max_age_ms: u64,
    /// How far from a handle a hand's grip may be for a grip press to take it, metres.
    pub grab_radius_m: f32,
    /// How far a held part of the holder's vehicle may be from the gun's centre to count as its
    /// handle, metres.
    pub handle_reach_m: f32,
    /// The shortest lever from the handles to the gun's pivot, metres. A gun whose handles the game
    /// does not place, or places at the pivot, swings about a pivot this far ahead of them.
    pub(crate) min_lever_m: f32,
    /// How near a hand must be to a mounted gun's handles or pivot to select it without the native
    /// mounted state, metres.
    pub(crate) nearby_reach_m: f32,
    /// How far past the hand's target the gun must sit before a pull tears it from its mount,
    /// metres.
    pub(crate) tear_lag_m: f32,
    /// The share of the hand's strongest pull the tear needs.
    pub(crate) tear_load: f32,
    /// How long the tearing pull must be held, seconds.
    pub(crate) tear_hold_s: f32,
    /// How long a torn-off gun takes to slide into the hand, seconds.
    pub(crate) take_slide_s: f32,
    /// How long after asking for the seat exit it is asked again if the player is still seated,
    /// milliseconds.
    pub(crate) seat_exit_retry_ms: u64,
    /// How long a tear from the seat waits for the player to leave it, milliseconds.
    pub(crate) seat_exit_timeout_ms: u64,
    pub recoil: MountedRecoilTuning,
}

impl MountedTuning {
    /// [`Self::max_age_ms`].
    pub fn max_age(&self) -> Window {
        Window::from_millis(self.max_age_ms)
    }

    /// [`Self::seat_exit_retry_ms`].
    pub(crate) fn seat_exit_retry(&self) -> Duration {
        Duration::from_millis(self.seat_exit_retry_ms)
    }

    /// [`Self::seat_exit_timeout_ms`].
    pub(crate) fn seat_exit_timeout(&self) -> Duration {
        Duration::from_millis(self.seat_exit_timeout_ms)
    }
}

/// The mounted gun's kick in the hands (see [`crate::vehicles::mounted_recoil`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountedRecoilTuning {
    /// How far a shot kicks the handles back along the barrel, metres.
    pub(crate) kick_back_m: f32,
    /// How far a shot kicks the handles down as the muzzle climbs, metres.
    pub(crate) kick_dip_m: f32,
    /// The handles' spring: stiff, so a single shot settles within a tenth of a second or so.
    pub stiffness: f32,
    pub damping_ratio: f32,
    /// How many shots' kick the handles can carry at once.
    pub(crate) max_shots: f32,
    /// The fastest the kick moves the handles, as the carried kick's reach covered per second.
    pub(crate) max_speed_per_s: f32,
}

/// Where the gunner stands to take a manned gun on foot (see
/// [`crate::vehicles::mounted::gunner_seat`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunnerSeatTuning {
    /// How far behind the handles, on the level, the gunner stands, metres.
    pub(crate) spot_behind_m: f32,
    /// How far from the gunner's spot, on the level, the head may be, metres.
    pub(crate) spot_radius_m: f32,
    /// How far below the handles the head may be and still stand at the spot, metres.
    pub(crate) spot_below_m: f32,
    /// How long the head stands at the spot before the entry starts, milliseconds, so a player
    /// walking across a vehicle's roof is not seated.
    pub(crate) dwell_ms: u64,
    /// How long the entry action is held at most, milliseconds: past the game's hold time for a
    /// seat change, and long enough for the entry to begin.
    pub(crate) hold_timeout_ms: u64,
}

impl GunnerSeatTuning {
    /// [`Self::dwell_ms`].
    pub fn dwell(&self) -> Duration {
        Duration::from_millis(self.dwell_ms)
    }

    /// [`Self::hold_timeout_ms`].
    pub(crate) fn hold_timeout(&self) -> Duration {
        Duration::from_millis(self.hold_timeout_ms)
    }
}

/// The driving view's correction of the driver's and the gunner's seat (see
/// [`crate::vehicles::driving_view`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrivingViewTuning {
    /// Where the hub sits from the eyes in the standard driving position, cockpit frame (+X right,
    /// +Y up, +Z back), metres. The standard is the middle of a road car's ergonomic range: the
    /// wheel's centre about a quarter to a third of a metre below the driver's eyes and about half
    /// a metre ahead of them. It is a little nearer than a road car's, because a VR rim holds no
    /// weight and a long reach tires the arms.
    pub(crate) hub_from_eyes_m: Vec3,
    /// Where a manned gun's handles' midpoint sits from the eyes in the standard gunner position,
    /// cockpit frame, metres: ahead of the chest and below the eyes, as a two-handled gun is held.
    pub(crate) handles_from_eyes_m: Vec3,
    /// The longest correction applied, metres; a longer one is shortened to this along its
    /// direction.
    pub(crate) max_correction_m: f32,
    /// How long after entering a vehicle the fallback wheel is carried into its space and the
    /// driving view begins measuring the hub, milliseconds: by then the driver has settled into
    /// the seat.
    pub(crate) settle_ms: u64,
    /// How long after the driver has settled the hub is averaged over, milliseconds.
    pub sample_window_ms: u64,
}

impl DrivingViewTuning {
    /// [`Self::settle_ms`].
    pub fn settle(&self) -> Duration {
        Duration::from_millis(self.settle_ms)
    }

    /// [`Self::sample_window_ms`].
    pub fn sample_window(&self) -> Duration {
        Duration::from_millis(self.sample_window_ms)
    }
}

/// Setting a cockpit's controls from the hands (see
/// [`crate::vehicles::physical::calibration`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationTuning {
    /// How long the player has to put the hands on the controls after a request, milliseconds.
    pub countdown_ms: u64,
    /// How long a request waits for the player to sit at a cockpit's controls before it is
    /// dropped, milliseconds.
    pub(crate) expire_after_ms: u64,
}

impl CalibrationTuning {
    /// [`Self::countdown_ms`].
    pub fn countdown(&self) -> Duration {
        Duration::from_millis(self.countdown_ms)
    }

    /// [`Self::expire_after_ms`].
    pub(crate) fn expire_after(&self) -> Duration {
        Duration::from_millis(self.expire_after_ms)
    }
}

impl MountedTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        millis("mounted.max_age_ms", self.max_age_ms, 10_000)?;
        positive("mounted.grab_radius_m", self.grab_radius_m, 1.0)?;
        positive("mounted.handle_reach_m", self.handle_reach_m, 10.0)?;
        positive("mounted.min_lever_m", self.min_lever_m, 5.0)?;
        positive("mounted.nearby_reach_m", self.nearby_reach_m, 10.0)?;
        positive("mounted.tear_lag_m", self.tear_lag_m, 5.0)?;
        range("mounted.tear_load", self.tear_load, 0.0, 1.0)?;
        nonnegative("mounted.tear_hold_s", self.tear_hold_s, 10.0)?;
        nonnegative("mounted.take_slide_s", self.take_slide_s, 5.0)?;
        millis(
            "mounted.seat_exit_retry_ms",
            self.seat_exit_retry_ms,
            60_000,
        )?;
        millis(
            "mounted.seat_exit_timeout_ms",
            self.seat_exit_timeout_ms,
            60_000,
        )?;
        let recoil = &self.recoil;
        nonnegative("mounted.recoil.kick_back_m", recoil.kick_back_m, 0.5)?;
        nonnegative("mounted.recoil.kick_dip_m", recoil.kick_dip_m, 0.5)?;
        positive("mounted.recoil.stiffness", recoil.stiffness, 100_000.0)?;
        positive("mounted.recoil.damping_ratio", recoil.damping_ratio, 10.0)?;
        positive("mounted.recoil.max_shots", recoil.max_shots, 100.0)?;
        positive(
            "mounted.recoil.max_speed_per_s",
            recoil.max_speed_per_s,
            1_000.0,
        )
    }
}

impl GunnerSeatTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        nonnegative("gunner_seat.spot_behind_m", self.spot_behind_m, 5.0)?;
        positive("gunner_seat.spot_radius_m", self.spot_radius_m, 5.0)?;
        nonnegative("gunner_seat.spot_below_m", self.spot_below_m, 5.0)?;
        millis("gunner_seat.dwell_ms", self.dwell_ms, 10_000)?;
        millis("gunner_seat.hold_timeout_ms", self.hold_timeout_ms, 60_000)
    }
}

impl DrivingViewTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        vec3("driving_view.hub_from_eyes_m", self.hub_from_eyes_m, 2.0)?;
        vec3(
            "driving_view.handles_from_eyes_m",
            self.handles_from_eyes_m,
            2.0,
        )?;
        nonnegative("driving_view.max_correction_m", self.max_correction_m, 2.0)?;
        millis("driving_view.settle_ms", self.settle_ms, 60_000)?;
        millis(
            "driving_view.sample_window_ms",
            self.sample_window_ms,
            60_000,
        )
    }
}

impl CalibrationTuning {
    pub(super) fn validate(&self) -> Result<(), Invalid> {
        millis("calibration.countdown_ms", self.countdown_ms, 60_000)?;
        millis(
            "calibration.expire_after_ms",
            self.expire_after_ms,
            3_600_000,
        )
    }
}
