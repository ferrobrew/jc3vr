//! A controller's scripted inputs at one moment, as the raw inputs and digital state the mod
//! publishes for an Index controller.
//!
//! The touches follow from the values (a deflected stick is touched, a pressed button is touched),
//! and the derived presses are read from the analog values with the payload's default press
//! thresholds and hysteresis, as the payload derives them live: the trigger press from the trigger
//! value, and the grip press from the squeeze force, since the Index's grip senses force.

use glam::Vec2;
use tracking_format::model::{Button, ButtonSet, HandInputs};

use crate::script::{HandTracks, PressButton};

/// The raw inputs of `hand` at `t` seconds.
pub fn sample(hand: &HandTracks, t: f32) -> HandInputs {
    let held = |button: PressButton| {
        hand.presses
            .iter()
            .any(|press| press.button == button && (press.from_s..press.until_s).contains(&t))
    };
    let stick = Vec2::new(hand.stick_x.sample(t), hand.stick_y.sample(t)).clamp_length_max(1.0);
    let trigger = hand.trigger.sample(t).clamp(0.0, 1.0);
    let thumbstick_click = held(PressButton::ThumbstickClick);
    let a_click = held(PressButton::A);
    let b_click = held(PressButton::B);
    HandInputs {
        trigger,
        trigger_click: trigger >= TRIGGER_CLICK,
        trigger_touch: trigger > TOUCH_EPSILON,
        squeeze: hand.squeeze.sample(t).clamp(0.0, 1.0),
        squeeze_force: hand.squeeze_force.sample(t).clamp(0.0, 1.0),
        thumbstick: stick,
        thumbstick_click,
        thumbstick_touch: thumbstick_click || stick.length() > TOUCH_EPSILON,
        trackpad: Vec2::ZERO,
        trackpad_force: 0.0,
        trackpad_touch: false,
        a_click,
        a_touch: a_click,
        b_click,
        b_touch: b_click,
        system_click: held(PressButton::System),
    }
}

/// The digital state of `inputs`, given the state on the frame before.
pub fn buttons(inputs: &HandInputs, previous: ButtonSet) -> ButtonSet {
    let mut set = ButtonSet::EMPTY;
    set.set(Button::TriggerClick, inputs.trigger_click);
    set.set(Button::TriggerTouch, inputs.trigger_touch);
    set.set(
        Button::TriggerPress,
        TRIGGER_PRESS.apply(previous.contains(Button::TriggerPress), inputs.trigger),
    );
    set.set(
        Button::SqueezePress,
        SQUEEZE_FORCE_PRESS.apply(
            previous.contains(Button::SqueezePress),
            inputs.squeeze_force,
        ),
    );
    set.set(Button::ThumbstickClick, inputs.thumbstick_click);
    set.set(Button::ThumbstickTouch, inputs.thumbstick_touch);
    set.set(Button::TrackpadTouch, inputs.trackpad_touch);
    set.set(Button::AClick, inputs.a_click);
    set.set(Button::ATouch, inputs.a_touch);
    set.set(Button::BClick, inputs.b_click);
    set.set(Button::BTouch, inputs.b_touch);
    set.set(Button::SystemClick, inputs.system_click);
    set
}

/// A press threshold with hysteresis: down at `press`, up below `release`.
#[derive(Debug, Copy, Clone)]
struct Thresholds {
    press: f32,
    release: f32,
}

impl Thresholds {
    fn apply(self, was_down: bool, value: f32) -> bool {
        value >= if was_down { self.release } else { self.press }
    }
}

/// The payload's default trigger press thresholds.
const TRIGGER_PRESS: Thresholds = Thresholds {
    press: 0.7,
    release: 0.55,
};

/// The payload's default squeeze force press thresholds.
const SQUEEZE_FORCE_PRESS: Thresholds = Thresholds {
    press: 0.3,
    release: 0.1,
};

/// The trigger value at which the Index's trigger clicks.
const TRIGGER_CLICK: f32 = 0.98;

/// The least value that reads as a finger on the input.
const TOUCH_EPSILON: f32 = 0.01;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        script::{HandTracks, Press},
        track::Track,
    };

    #[test]
    fn a_deflected_stick_is_touched_and_capped_at_full() {
        let hand = HandTracks {
            stick_x: Track::start(1.0),
            stick_y: Track::start(1.0),
            ..HandTracks::default()
        };
        let inputs = sample(&hand, 0.0);
        assert!(inputs.thumbstick_touch);
        assert!((inputs.thumbstick.length() - 1.0).abs() < 1e-6);
        assert!(!sample(&HandTracks::default(), 0.0).thumbstick_touch);
    }

    #[test]
    fn a_press_holds_its_button_and_touch_for_its_interval() {
        let hand = HandTracks {
            presses: vec![Press {
                button: PressButton::A,
                from_s: 1.0,
                until_s: 2.0,
            }],
            ..HandTracks::default()
        };
        assert!(!sample(&hand, 0.5).a_click);
        let inputs = sample(&hand, 1.5);
        assert!(inputs.a_click && inputs.a_touch && !inputs.b_click);
        assert!(!sample(&hand, 2.0).a_click);
    }

    #[test]
    fn the_trigger_press_has_hysteresis() {
        let at = |trigger: f32| HandInputs {
            trigger,
            ..HandInputs::default()
        };
        let down = buttons(&at(0.75), ButtonSet::EMPTY);
        assert!(down.contains(Button::TriggerPress));
        // Between the thresholds, the press keeps what it was.
        assert!(buttons(&at(0.6), down).contains(Button::TriggerPress));
        assert!(!buttons(&at(0.6), ButtonSet::EMPTY).contains(Button::TriggerPress));
        assert!(!buttons(&at(0.5), down).contains(Button::TriggerPress));
    }

    #[test]
    fn a_resting_hand_round_the_grip_does_not_press_it() {
        let inputs = sample(&HandTracks::default(), 0.0);
        assert!(inputs.squeeze > 0.8);
        assert!(!buttons(&inputs, ButtonSet::EMPTY).contains(Button::SqueezePress));
    }
}
