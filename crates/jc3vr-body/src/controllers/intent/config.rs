//! The controller-to-game bindings' settings.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::intent::tuning::CONTROLS;

/// The controller-to-game bindings' settings, at `controllers.bindings` in the settings file; the
/// payload binds the slot at startup.
pub static BINDINGS_CONFIG: ConfigSlot<BindingsConfig> = ConfigSlot::new("controllers.bindings");

/// The trigger pull below which an analog trigger action reads as zero (the controls tuning's
/// `bindings.trigger_deadzone`).
pub fn trigger_deadzone() -> f32 {
    CONTROLS.get().bindings.trigger_deadzone
}

/// How long the system button must be held to open the commlink rather than pause, seconds (the
/// controls tuning's `bindings.long_press_s`).
pub fn long_press_s() -> f32 {
    CONTROLS.get().bindings.long_press_s
}

/// How far down the dominant stick must be flicked on foot to toggle the seated crouch (the
/// controls tuning's `bindings.crouch_flick`).
pub fn crouch_flick() -> f32 {
    CONTROLS.get().bindings.crouch_flick
}

/// How far either stick must be pushed along one axis to step a menu (the controls tuning's
/// `bindings.menu_stick_push`).
pub fn menu_stick_push() -> f32 {
    CONTROLS.get().bindings.menu_stick_push
}

/// How long a lone stick click waits for the other before it acts, seconds (the controls tuning's
/// `bindings.stick_chord_window_s`).
pub fn stick_chord_window_s() -> f32 {
    CONTROLS.get().bindings.stick_chord_window_s
}

/// How long both stick clicks must be held together to recenter the VR view, seconds (the
/// controls tuning's `bindings.recenter_hold_s`).
pub fn recenter_hold_s() -> f32 {
    CONTROLS.get().bindings.recenter_hold_s
}

/// How soon after the left stick click a fresh right B press can show or hide the debug panel,
/// seconds (the controls tuning's `bindings.panel_chord_window_s`).
pub fn panel_chord_window_s() -> f32 {
    CONTROLS.get().bindings.panel_chord_window_s
}

/// The controller-to-game bindings (see the `bindings` module), which inject only under the
/// controller scheme while a controller is active.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BindingsConfig {
    /// The radial deadzone of the stick that moves, steers, and navigates, `0..1`. The output is
    /// rescaled so that it starts from zero at the deadzone's edge.
    pub move_deadzone: f32,
}

impl BindingsConfig {
    pub const fn new() -> Self {
        Self {
            move_deadzone: 0.15,
        }
    }
}

impl Default for BindingsConfig {
    fn default() -> Self {
        Self::new()
    }
}
