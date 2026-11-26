//! The roomscale's tuning, read from the shipped `tuning/roomscale.toml` ([`ROOMSCALE`]): the
//! automatic posture's detection and the seated crouch, the seated eye reference, the crouch's
//! hysteresis, the comfort fade's timing, the seat fade's hold, the stick's deflection that counts
//! as moving, and the driver's view correction's ease.

use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::roomscale::{
    fade::FadeTuning, posture::AutoPostureTuning, seated::SeatedReferenceTuning,
};

jc3vr_core::tuning_file! {
    /// The roomscale's tuning file.
    pub static ROOMSCALE: RoomscaleTuning = "roomscale.toml";
}

/// The roomscale tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomscaleTuning {
    pub posture: AutoPostureTuning,
    pub seated: SeatedReferenceTuning,
    pub crouch: CrouchTuning,
    pub fade: FadeTuning,
    pub seat: SeatTuning,
    /// The move stick past this deflection moves the body, for the yaw's deadzone, `0..1`.
    pub moving_stick: f32,
    pub driving_view: DrivingViewTuning,
}

impl Tuning for RoomscaleTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let p = &self.posture;
        nonnegative("posture.stand_after_s", p.stand_after_s, 600.0)?;
        nonnegative("posture.sit_after_s", p.sit_after_s, 600.0)?;
        nonnegative("posture.seated_crouch_drop_m", p.seated_crouch_drop_m, 2.0)?;
        range(
            "posture.seated_eye_fraction",
            p.seated_eye_fraction,
            0.1,
            1.0,
        )?;
        nonnegative("posture.min_eye_gap_m", p.min_eye_gap_m, 1.0)?;
        range("posture.sit_fraction", p.sit_fraction, 0.0, 1.0)?;
        range(
            "posture.stand_fraction",
            p.stand_fraction,
            p.sit_fraction,
            1.0,
        )?;
        range(
            "posture.floor_margin_fraction",
            p.floor_margin_fraction,
            0.0,
            1.0,
        )?;
        nonnegative("posture.steady_band_m", p.steady_band_m, 1.0)?;

        let s = &self.seated;
        range("seated.window_s", s.window_s, 10.0, 3600.0)?;
        nonnegative("seated.rate_mps", s.rate_mps, 1.0)?;
        nonnegative("seated.deadband_m", s.deadband_m, 1.0)?;
        range("seated.settle_fraction", s.settle_fraction, 0.0, 1.0)?;
        positive("seated.max_step_s", s.max_step_s, 10.0)?;

        let c = &self.crouch;
        nonnegative("crouch.enter_drop_m", c.enter_drop_m, 2.0)?;
        range("crouch.exit_drop_m", c.exit_drop_m, 0.0, c.enter_drop_m)?;

        nonnegative("fade.out_s", self.fade.out_s, 10.0)?;
        nonnegative("fade.in_s", self.fade.in_s, 10.0)?;
        nonnegative("seat.fade_hold_s", self.seat.fade_hold_s, 10.0)?;
        range("moving_stick", self.moving_stick, 0.0, 1.0)?;
        nonnegative("driving_view.blend_s", self.driving_view.blend_s, 10.0)
    }
}

/// The crouch's hysteresis on the head's drop
/// ([`crate::controllers::roomscale::crouch::CrouchTracker`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrouchTuning {
    /// How far (metres, at the character's scale) the head must drop below the standing eye height
    /// before the character counts as crouched, for the capsule and the readouts.
    pub enter_drop_m: f32,
    /// How far the head must come back up, metres below the standing eye height at the character's
    /// scale, before the character counts as standing again. At most the entry depth, so the
    /// hysteresis never inverts.
    pub exit_drop_m: f32,
}

/// The seat transitions ([`crate::controllers::roomscale::seat`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatTuning {
    /// Under [`SeatTransition::Fade`](crate::controllers::roomscale::config::SeatTransition::Fade),
    /// how long the view stays dark after the seat locks, so that the re-based view settles before
    /// it clears, seconds.
    pub fade_hold_s: f32,
}

/// The driver's view correction ([`crate::controllers::facts::driving`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrivingViewTuning {
    /// How long the view takes to ease to a new correction, seconds.
    pub blend_s: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(ROOMSCALE.get().validate(), Ok(()));
    }
}
