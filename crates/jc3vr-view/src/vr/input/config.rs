//! What the VR runtime's input sampling reads from the settings: the raw input interpretation
//! ([`InputConfig`], stored as `controllers.input`), and the frame's [`SamplingConfig`].

use jc3vr_core::config::engine::ConfigSlot;
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

/// The trigger values that press and release
/// [`Button::TriggerPress`](jc3vr_input::frame::Button::TriggerPress).
pub const TRIGGER_PRESS: PressThresholds = PressThresholds {
    press: 0.7,
    release: 0.55,
};

/// The squeeze values that press and release
/// [`Button::SqueezePress`](jc3vr_input::frame::Button::SqueezePress) on a controller without a
/// force-sensing grip (Touch, or a profile the runtime emulates). The grip is used only as an
/// edge, so the press threshold sits high enough that resting a hand on the grip does not press
/// it.
pub const SQUEEZE_PRESS: PressThresholds = PressThresholds {
    press: 0.75,
    release: 0.5,
};

/// The squeeze forces that press and release
/// [`Button::SqueezePress`](jc3vr_input::frame::Button::SqueezePress) on a controller whose
/// profile provides the grip force (the Index). The Index grip's value is capacitive and reads a
/// full squeeze with the hand merely closed round the handle, so the press waits for a deliberate
/// squeeze, and the release sits low enough that a held grab does not need a hard squeeze to keep.
pub const SQUEEZE_FORCE_PRESS: PressThresholds = PressThresholds {
    press: 0.3,
    release: 0.1,
};

/// The trackpad forces that press and release
/// [`Button::TrackpadPressLeft`](jc3vr_input::frame::Button::TrackpadPressLeft) and
/// [`Button::TrackpadPressRight`](jc3vr_input::frame::Button::TrackpadPressRight), on a
/// controller with a trackpad. A resting thumb reads no force, so a light press is enough.
pub const TRACKPAD_PRESS: PressThresholds = PressThresholds {
    press: 0.4,
    release: 0.2,
};

/// How `hand`'s grip presses on `profile`: by its force where the profile provides one, and
/// otherwise by its value.
pub fn grip_press(profile: InteractionProfile, hand: Hand) -> GripPress {
    if provides(profile, hand, Input::Float(FloatInput::SqueezeForce)) {
        GripPress::Force(SQUEEZE_FORCE_PRESS)
    } else {
        GripPress::Value(SQUEEZE_PRESS)
    }
}

/// The press rules of `hand` on `profile`.
pub fn press_rules(profile: InteractionProfile, hand: Hand) -> PressRules {
    PressRules {
        trigger: TRIGGER_PRESS,
        grip: grip_press(profile, hand),
        trackpad: TRACKPAD_PRESS,
    }
}

/// Raw input interpretation.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// Locate the hand skeleton through `XR_EXT_hand_tracking` when the runtime offers it. The
    /// joints feed the finger curls; with this off, or without runtime support, the curls are
    /// estimated from the inputs.
    pub hand_tracking: bool,
    /// Run the input self-check once per session, logging how the runtime treats the action
    /// layout (see `crate::vr::input::self_check`).
    pub self_check: bool,
}

impl InputConfig {
    pub const fn new() -> Self {
        Self {
            hand_tracking: true,
            self_check: true,
        }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self::new()
    }
}
