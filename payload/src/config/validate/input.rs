//! Checks for raw input interpretation and the controller-to-game bindings.

use crate::{
    config::validate::{Invalid, positive, press_thresholds, range},
    controllers::config::{BindingsConfig, InputConfig},
};

/// Check the raw input settings. Paths are relative to `controllers.input`.
pub(super) fn input(input: &InputConfig) -> Result<(), Invalid> {
    press_thresholds("input.trigger_press", input.trigger_press)?;
    press_thresholds("input.squeeze_press", input.squeeze_press)?;
    press_thresholds("input.squeeze_force_press", input.squeeze_force_press)?;
    press_thresholds("input.trackpad_press", input.trackpad_press)
}

/// Check the controller-to-game bindings. Paths are relative to `controllers.bindings`.
pub(super) fn bindings(c: &BindingsConfig) -> Result<(), Invalid> {
    range("bindings.move_deadzone", c.move_deadzone, 0.0, 1.0)?;
    range("bindings.trigger_deadzone", c.trigger_deadzone, 0.0, 1.0)?;
    positive("bindings.long_press_s", c.long_press_s, 60.0)?;
    range("bindings.crouch_flick", c.crouch_flick, 0.0, 1.0)?;
    range("bindings.menu_stick_push", c.menu_stick_push, 0.0, 1.0)?;
    positive("bindings.stick_chord_window_s", c.stick_chord_window_s, 1.0)?;
    positive("bindings.recenter_hold_s", c.recenter_hold_s, 60.0)?;
    positive("bindings.panel_chord_window_s", c.panel_chord_window_s, 1.0)
}
