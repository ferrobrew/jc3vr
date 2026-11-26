//! The controller-to-game bindings' settings.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

/// The controller-to-game bindings' settings, at `controllers.bindings` in the settings file; the
/// payload binds the slot at startup.
pub static BINDINGS_CONFIG: ConfigSlot<BindingsConfig> = ConfigSlot::new("controllers.bindings");

/// The trigger pull below which an analog trigger action (throttle, collective, thrust) reads as
/// zero, `0..1`.
pub const TRIGGER_DEADZONE: f32 = 0.05;
/// How long the system button must be held to open the commlink rather than pause, seconds.
pub const LONG_PRESS_S: f32 = 0.5;
/// How far down the dominant stick must be flicked on foot to toggle the seated crouch, `0..1`.
/// The stick must come back above half of it before the next flick counts.
pub const CROUCH_FLICK: f32 = 0.8;
/// How far either stick must be pushed along one axis to step a menu, `0..1`. The step holds, and
/// the game repeats it, until the push falls back below half of it.
pub const MENU_STICK_PUSH: f32 = 0.5;
/// How long a lone stick click waits for the other before it acts, seconds: both clicks pressed
/// within it are the recenter chord, and neither acts on its own.
pub const STICK_CHORD_WINDOW_S: f32 = 0.1;
/// How long both stick clicks must be held together to recenter the VR view, seconds.
pub const RECENTER_HOLD_S: f32 = 1.0;
/// How soon after the left stick click a fresh right B press can show or hide the debug panel,
/// seconds. The left click waits for the longer of this and the recenter join window.
pub const PANEL_CHORD_WINDOW_S: f32 = 0.35;

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
