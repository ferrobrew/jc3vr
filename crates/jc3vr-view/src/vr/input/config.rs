//! What the VR runtime's input sampling reads from the settings: the raw input interpretation
//! ([`InputConfig`], stored as `controllers.input`), and the frame's [`SamplingConfig`].

use jc3vr_core::config::engine::ConfigSlot;
use jc3vr_input::frame::{GripPress, Hand, InteractionProfile, PressRules};
use serde::{Deserialize, Serialize};

use crate::{
    tuning::VIEW,
    vr::input::layout::{FloatInput, Input, provides},
};

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

/// How `hand`'s grip presses on `profile`: by its force where the profile provides one, and
/// otherwise by its value.
pub fn grip_press(profile: InteractionProfile, hand: Hand) -> GripPress {
    let press = &VIEW.get().press;
    if provides(profile, hand, Input::Float(FloatInput::SqueezeForce)) {
        GripPress::Force(press.squeeze_force)
    } else {
        GripPress::Value(press.squeeze)
    }
}

/// The press rules of `hand` on `profile`.
pub fn press_rules(profile: InteractionProfile, hand: Hand) -> PressRules {
    let press = &VIEW.get().press;
    PressRules {
        trigger: press.trigger,
        grip: grip_press(profile, hand),
        trackpad: press.trackpad,
        trackpad_half_margin: press.trackpad_half_margin,
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
}

impl InputConfig {
    pub const fn new() -> Self {
        Self {
            hand_tracking: true,
        }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self::new()
    }
}
