//! Checks for raw input interpretation, the controller-to-game bindings, and the mode detector's
//! debounce.

use crate::{
    config::validate::{Invalid, nonnegative, positive, press_thresholds, range},
    controllers::config::{BindingsConfig, InputConfig, ModeConfig},
};

/// Check the raw input settings. Paths are relative to `controllers.input`.
pub(super) fn input(input: &InputConfig) -> Result<(), Invalid> {
    press_thresholds("input.trigger_press", input.trigger_press)?;
    press_thresholds("input.squeeze_press", input.squeeze_press)?;
    press_thresholds("input.squeeze_force_press", input.squeeze_force_press)?;
    press_thresholds("input.trackpad_press", input.trackpad_press)?;
    range("input.stick_deadzone", input.stick_deadzone, 0.0, 1.0)
}

/// Check the controller-to-game bindings. Paths are relative to `controllers.bindings`.
pub(super) fn bindings(c: &BindingsConfig) -> Result<(), Invalid> {
    range("bindings.move_deadzone", c.move_deadzone, 0.0, 1.0)?;
    range("bindings.trigger_deadzone", c.trigger_deadzone, 0.0, 1.0)?;
    positive("bindings.long_press_s", c.long_press_s, 60.0)?;
    range("bindings.crouch_flick", c.crouch_flick, 0.0, 1.0)?;
    range("bindings.menu_stick_push", c.menu_stick_push, 0.0, 1.0)?;
    nonnegative("bindings.stick_chord_window_s", c.stick_chord_window_s, 1.0)?;
    positive("bindings.recenter_hold_s", c.recenter_hold_s, 60.0)?;
    nonnegative("bindings.panel_chord_window_s", c.panel_chord_window_s, 1.0)?;
    positive("bindings.yank.min_distance_m", c.yank.min_distance_m, 10.0)?;
    positive("bindings.yank.min_speed_mps", c.yank.min_speed_mps, 100.0)?;
    positive("bindings.yank.window_s", c.yank.window_s, 60.0)?;
    nonnegative("bindings.yank.cooldown_s", c.yank.cooldown_s, 60.0)?;
    nonnegative("bindings.yank.body_drop_m", c.yank.body_drop_m, 10.0)
}

/// Check the mode detector's debounce. Paths are relative to `controllers.mode`.
pub(super) fn mode(c: &ModeConfig) -> Result<(), Invalid> {
    if c.debounce_ticks == 0 || c.debounce_ticks > 60 {
        return Err(Invalid(
            "mode.debounce_ticks must be between 1 and 60".into(),
        ));
    }
    Ok(())
}
