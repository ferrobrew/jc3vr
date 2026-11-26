//! What the VR runtime's input sampling reads from the settings: the raw input interpretation
//! ([`InputConfig`], stored as `controllers.input`), and the frame's [`SamplingConfig`].

use jc3vr_core::config::engine::{ConfigSlot, Invalid, range};
use jc3vr_input::frame::{GripPress, Hand, InteractionProfile, PressRules, PressThresholds};
use serde::{Deserialize, Serialize};

use crate::vr::input::layout::{FloatInput, Input, provides};

/// The raw input interpretation, at `controllers.input` in the settings file. The payload binds
/// this slot and the four below to their fields in the controller settings at startup.
pub static INPUT_CONFIG: ConfigSlot<InputConfig> = ConfigSlot::new("controllers.input");

/// The controllers master switch, `controllers.enabled`.
pub static CONTROLLERS_ENABLED: ConfigSlot<bool> = ConfigSlot::new("controllers.enabled");

/// The hand that holds and fires the guns, `controllers.dominant_hand`.
pub static DOMINANT_HAND: ConfigSlot<Hand> = ConfigSlot::new("controllers.dominant_hand");

/// The haptic output's switch, `controllers.haptics.enabled`.
pub static HAPTICS_ENABLED: ConfigSlot<bool> = ConfigSlot::new("controllers.haptics.enabled");

/// The haptic output's strength, `controllers.haptics.intensity`.
pub static HAPTICS_INTENSITY: ConfigSlot<f32> = ConfigSlot::new("controllers.haptics.intensity");

/// What one frame's input update reads from the controller settings: whether the controllers are
/// on, the raw input interpretation, and the haptic output.
#[derive(Copy, Clone)]
pub(crate) struct SamplingConfig {
    /// The controllers master switch: off, the update publishes nothing and drops the queued
    /// haptics.
    pub enabled: bool,
    pub input: InputConfig,
    pub haptics: HapticOutput,
}

/// The haptic output's switch and strength, from the haptics settings.
#[derive(Copy, Clone)]
pub(crate) struct HapticOutput {
    /// Off, nothing vibrates.
    pub enabled: bool,
    /// Scales every vibration's amplitude, `0..=1`.
    pub intensity: f32,
}

/// Raw input interpretation.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// The trigger values that press and release
    /// [`Button::TriggerPress`](jc3vr_input::frame::Button::TriggerPress).
    pub trigger_press: PressThresholds,
    /// The squeeze values that press and release
    /// [`Button::SqueezePress`](jc3vr_input::frame::Button::SqueezePress) on a controller
    /// without a force-sensing grip (Touch, or a profile the runtime emulates). The grip is used
    /// only as an edge, so the press threshold sits high enough that resting a hand on the grip
    /// does not press it.
    pub squeeze_press: PressThresholds,
    /// The squeeze forces that press and release
    /// [`Button::SqueezePress`](jc3vr_input::frame::Button::SqueezePress) on a controller
    /// whose profile provides the grip force (the Index). The Index grip's value is capacitive and
    /// reads a full squeeze with the hand merely closed round the handle, so the press waits for a
    /// deliberate squeeze, and the release sits low enough that a held grab does not need a hard
    /// squeeze to keep.
    pub squeeze_force_press: PressThresholds,
    /// The trackpad forces that press and release
    /// [`Button::TrackpadPressLeft`](jc3vr_input::frame::Button::TrackpadPressLeft) and
    /// [`Button::TrackpadPressRight`](jc3vr_input::frame::Button::TrackpadPressRight), on
    /// a controller with a trackpad. A resting thumb reads no force, so a light press is enough.
    pub trackpad_press: PressThresholds,
    /// Locate the hand skeleton through `XR_EXT_hand_tracking` when the runtime offers it. The
    /// joints feed the finger curls; with this off, or without runtime support, the curls are
    /// estimated from the inputs.
    pub hand_tracking: bool,
    /// Run the input self-check once per session, logging how the runtime treats the action
    /// layout (see `crate::vr::input::self_check`).
    pub self_check: bool,
}

impl InputConfig {
    /// Check the raw input settings, returning the first that is out of range. Paths are relative
    /// to `controllers`.
    pub fn validate(&self) -> Result<(), Invalid> {
        validate_thresholds(self.trigger_press, "input.trigger_press")?;
        validate_thresholds(self.squeeze_press, "input.squeeze_press")?;
        validate_thresholds(self.squeeze_force_press, "input.squeeze_force_press")?;
        validate_thresholds(self.trackpad_press, "input.trackpad_press")
    }

    pub const fn new() -> Self {
        Self {
            trigger_press: PressThresholds {
                press: 0.7,
                release: 0.55,
            },
            squeeze_press: PressThresholds {
                press: 0.75,
                release: 0.5,
            },
            squeeze_force_press: PressThresholds {
                press: 0.3,
                release: 0.1,
            },
            trackpad_press: PressThresholds {
                press: 0.4,
                release: 0.2,
            },
            hand_tracking: true,
            self_check: true,
        }
    }

    /// What derives `hand`'s grip press under `profile`: the squeeze force where the profile
    /// provides it, and the squeeze value elsewhere.
    pub fn grip_press(&self, profile: InteractionProfile, hand: Hand) -> GripPress {
        if provides(profile, hand, Input::Float(FloatInput::SqueezeForce)) {
            GripPress::Force(self.squeeze_force_press)
        } else {
            GripPress::Value(self.squeeze_press)
        }
    }

    /// How `hand`'s derived presses are read under `profile`.
    pub fn press_rules(&self, profile: InteractionProfile, hand: Hand) -> PressRules {
        PressRules {
            trigger: self.trigger_press,
            grip: self.grip_press(profile, hand),
            trackpad: self.trackpad_press,
        }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Check that both thresholds are in `[0, 1]` and the release does not exceed the press. `name` is
/// the pair's settings path, which leads the failure's message.
pub fn validate_thresholds(pair: PressThresholds, name: &str) -> Result<(), Invalid> {
    range(&format!("{name}.press"), pair.press, 0.0, 1.0)?;
    range(&format!("{name}.release"), pair.release, 0.0, 1.0)?;
    if pair.release > pair.press {
        return Err(Invalid(format!("{name}.release exceeds press")));
    }
    Ok(())
}
