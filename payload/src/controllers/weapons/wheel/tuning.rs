//! The weapon wheel's fixed tuning, and the feature switch it takes from the configuration
//! ([`WheelConfig`]).

use crate::controllers::weapons::wheel::config::WheelConfig;

/// The weapon wheel's tuning, switched off as [`WheelConfig`] ships it.
pub const WHEEL: WheelTuning = WheelTuning::new();

/// The weapon wheel (see [`crate::controllers::weapons::wheel`]): how it opens, how the hand steers
/// it, and where the cross is drawn.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct WheelTuning {
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
    /// How far the hand must turn from where the wheel opened to leave the centre, degrees: the
    /// hand chooses an arm by pointing toward it.
    pub point_leave_deg: f32,
    /// How far back toward where the wheel opened the hand must turn to return to the centre,
    /// degrees. Below [`point_leave_deg`](Self::point_leave_deg).
    pub point_return_deg: f32,
    /// How far past the line between two arms the hand must go before the other arm is chosen,
    /// degrees around the wheel.
    pub hysteresis_deg: f32,
    /// Move the game's cross on the floating panel to where the steering hand was, seen from the
    /// head, when the wheel opened, instead of over the character's hips.
    pub place_at_hand: bool,
}

impl WheelTuning {
    const fn new() -> Self {
        Self {
            enabled: false,
            trackpad: true,
            stick: true,
            stick_open: 0.7,
            stick_release: 0.4,
            point_leave_deg: 12.0,
            point_return_deg: 7.0,
            hysteresis_deg: 10.0,
            place_at_hand: true,
        }
    }

    /// The fixed tuning with the switch `config` sets.
    pub const fn of(config: &WheelConfig) -> Self {
        Self {
            enabled: config.enabled,
            ..WHEEL
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_ships_off_and_its_switch_still_turns_it_on() {
        assert!(!WheelConfig::default().enabled);
        assert!(!WheelTuning::of(&WheelConfig::default()).enabled);
        assert!(WheelTuning::of(&WheelConfig { enabled: true }).enabled);
    }
}
