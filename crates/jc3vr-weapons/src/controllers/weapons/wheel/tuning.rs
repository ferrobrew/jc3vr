//! The weapon wheel's tuning, a section of the inventory's tuning file
//! ([`INVENTORY`](crate::controllers::weapons::inventory::tuning::INVENTORY)'s `wheel`). Whether
//! the wheel acts at all is a setting
//! ([`WheelConfig`](crate::controllers::weapons::wheel::config::WheelConfig)).

use jc3vr_core::config::{Invalid, positive, range};
use serde::Deserialize;

/// The weapon wheel (see [`crate::controllers::weapons::wheel`]): how its openers open it and how
/// the hand steers it.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelTuning {
    /// How far the `weapon_wheel` action must be pushed to open the wheel, `0..=1`.
    pub(crate) stick_open: f32,
    /// How far the `weapon_wheel` action must stay pushed to keep the wheel open, `0..=1`. Below
    /// [`stick_open`](Self::stick_open), so that a push held near the threshold does not flicker.
    pub(crate) stick_release: f32,
    /// How far the hand must turn from where the wheel opened to leave the centre, degrees: the
    /// hand chooses an arm by pointing toward it.
    pub(crate) point_leave_deg: f32,
    /// How far back toward where the wheel opened the hand must turn to return to the centre,
    /// degrees. Below [`point_leave_deg`](Self::point_leave_deg).
    pub(crate) point_return_deg: f32,
    /// How far past the line between two arms the hand must go before the other arm is chosen,
    /// degrees around the wheel.
    pub hysteresis_deg: f32,
}

impl WheelTuning {
    pub(crate) fn validate(&self) -> Result<(), Invalid> {
        range("wheel.stick_open", self.stick_open, 0.05, 1.0)?;
        range(
            "wheel.stick_release",
            self.stick_release,
            0.0,
            self.stick_open,
        )?;
        positive("wheel.point_leave_deg", self.point_leave_deg, 90.0)?;
        range(
            "wheel.point_return_deg",
            self.point_return_deg,
            0.0,
            self.point_leave_deg,
        )?;
        range("wheel.hysteresis_deg", self.hysteresis_deg, 0.0, 44.0)
    }
}
