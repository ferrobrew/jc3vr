//! The weapon wheel's settings, held in
//! [`WeaponsConfig::wheel`](crate::controllers::config::WeaponsConfig::wheel).

use serde::{Deserialize, Serialize};

/// The weapon wheel (see [`crate::controllers::weapons::wheel`]). It acts only under the controller
/// scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WheelConfig {
    /// Open the game's selection cross with a held input and choose an arm with the hand.
    pub enabled: bool,
    /// Open the wheel with the weapon-select trackpad press (`weapon_select_press`, the off-hand
    /// trackpad pressed down on the Index). A press in a quadrant starts on that quadrant's arm, so
    /// a tap still selects it at once.
    pub trackpad: bool,
    /// Open the wheel with the `weapon_wheel` action (the dominant stick held up by default).
    pub stick: bool,
    /// How far the `weapon_wheel` action must be pushed to open the wheel, `0..=1`.
    pub stick_open: f32,
    /// How far the `weapon_wheel` action must stay pushed to keep the wheel open, `0..=1`. Below
    /// [`stick_open`](Self::stick_open), so that a push held near the threshold does not flicker.
    pub stick_release: f32,
    /// How the hand chooses an arm.
    pub steer: SteerMode,
    /// How far the hand must turn from where the wheel opened to leave the centre, degrees
    /// ([`SteerMode::Point`]).
    pub point_leave_deg: f32,
    /// How far back toward where the wheel opened the hand must turn to return to the centre,
    /// degrees ([`SteerMode::Point`]). Below [`point_leave_deg`](Self::point_leave_deg).
    pub point_return_deg: f32,
    /// How far the hand must move from where the wheel opened to leave the centre, metres
    /// ([`SteerMode::Offset`]).
    pub offset_leave_m: f32,
    /// How close to where the wheel opened the hand must come back to return to the centre,
    /// metres ([`SteerMode::Offset`]). Below [`offset_leave_m`](Self::offset_leave_m).
    pub offset_return_m: f32,
    /// How far past the line between two arms the hand must go before the other arm is chosen,
    /// degrees around the wheel.
    pub hysteresis_deg: f32,
    /// Move the game's cross on the floating panel to where the steering hand was, seen from the
    /// head, when the wheel opened, instead of over the character's hips.
    pub place_at_hand: bool,
}

impl WheelConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            trackpad: true,
            stick: true,
            stick_open: 0.7,
            stick_release: 0.4,
            steer: SteerMode::Point,
            point_leave_deg: 12.0,
            point_return_deg: 7.0,
            offset_leave_m: 0.045,
            offset_return_m: 0.025,
            hysteresis_deg: 10.0,
            place_at_hand: true,
        }
    }

    /// The leave and return distances of `self.steer`, in its units.
    pub fn deadzone(&self) -> (f32, f32) {
        match self.steer {
            SteerMode::Point => (self.point_leave_deg, self.point_return_deg),
            SteerMode::Offset => (self.offset_leave_m, self.offset_return_m),
        }
    }
}

impl Default for WheelConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// How the steering hand chooses an arm of the wheel.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SteerMode {
    /// Point the hand toward the arm: the turn of the controller's aim from where it pointed when
    /// the wheel opened.
    #[default]
    Point,
    /// Move the hand toward the arm: the controller's offset, in the plane facing the head, from
    /// where it was when the wheel opened.
    Offset,
}
