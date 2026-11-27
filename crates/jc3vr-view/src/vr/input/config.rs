//! What the VR runtime's input sampling reads from the settings: the controllers master switch,
//! the dominant hand, the raw input interpretation ([`InputConfig`]), the vibration settings
//! ([`HapticsConfig`]), the press rules each profile's inputs are read by, and the frame's
//! [`SamplingConfig`].

use jc3vr_core::config::{ConfigSlot, SlotOptions};
use jc3vr_input::{
    frame::{GripPress, Hand, InteractionProfile, PressRules},
    haptics::cues::Category,
};
use serde::{Deserialize, Serialize};

use crate::{
    tuning::VIEW,
    vr::input::layout::{FloatInput, Input, provides},
};

/// The raw input interpretation, at `controllers.input` in the settings file. The controller
/// settings bind this slot and the three below to their fields.
pub static INPUT_CONFIG: ConfigSlot<InputConfig> = ConfigSlot::new("controllers.input");

/// The controllers master switch, `controllers.enabled`. Off, the mod reads no controller input and
/// publishes no controller frame, so every controller subsystem is inert and the gamepad scheme is
/// in effect.
pub static CONTROLLERS_ENABLED: ConfigSlot<bool> = ConfigSlot::with(
    "controllers.enabled",
    SlotOptions::new().defaults_to(|| true),
);

/// The hand that holds and fires the guns, `controllers.dominant_hand`.
pub static DOMINANT_HAND: ConfigSlot<Hand> = ConfigSlot::new("controllers.dominant_hand");

/// The controller vibration settings, at `controllers.haptics` in the settings file.
pub static HAPTICS_CONFIG: ConfigSlot<HapticsConfig> = ConfigSlot::new("controllers.haptics");

/// Controller vibration, as `controllers.haptics`. The primitives' shapes, the mixer's rules, and
/// the cues are tuning ([`HAPTICS`](crate::tuning::HAPTICS)); every category of the mod's own cues
/// plays, each with its primitive's frequency.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HapticsConfig {
    /// Off, nothing vibrates.
    pub enabled: bool,
    /// Scales every vibration's amplitude, `0..=1`.
    pub intensity: f32,
    /// Forward the game's rumble to the controllers: a weapon's own fire rumble to the hand holding
    /// that gun, and the game's per-frame left and right motor strength to the hands by side.
    pub game_rumble: bool,
}

impl HapticsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            intensity: 1.0,
            game_rumble: true,
        }
    }

    /// Whether `category` plays, the master switch aside: the game's rumble by its switch, and
    /// every category of the mod's own cues always.
    pub const fn category(&self, category: Category) -> bool {
        match category {
            Category::Game => self.game_rumble,
            Category::Contact
            | Category::Weapons
            | Category::Vehicles
            | Category::Traversal
            | Category::Interface => true,
        }
    }
}

impl Default for HapticsConfig {
    fn default() -> Self {
        Self::new()
    }
}

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
