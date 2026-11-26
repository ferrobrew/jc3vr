//! The controller-to-game bindings' settings.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::intent::yank::GrappleYankConfig;

/// The controller-to-game bindings' settings, at `controllers.bindings` in the settings file; the
/// payload binds the slot at startup.
pub static BINDINGS_CONFIG: ConfigSlot<BindingsConfig> = ConfigSlot::new("controllers.bindings");

/// The controller-to-game bindings (see the `bindings` module). They inject only under
/// the controller scheme while a controller is active, so they default on.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BindingsConfig {
    /// Inject the controllers' input into the game's actions.
    pub enabled: bool,
    /// The radial deadzone of the stick that moves, steers, and navigates, `0..1`. The output is
    /// rescaled so that it starts from zero at the deadzone's edge.
    pub move_deadzone: f32,
    /// The trigger pull below which an analog trigger action (throttle, collective, thrust) reads
    /// as zero, `0..1`.
    pub trigger_deadzone: f32,
    /// How long the system button must be held to open the commlink rather than pause, seconds.
    pub long_press_s: f32,
    /// How far down the dominant stick must be flicked on foot to toggle the seated crouch, `0..1`.
    /// The stick must come back above half of it before the next flick counts.
    pub crouch_flick: f32,
    /// How far either stick must be pushed along one axis to step a menu, `0..1`. The step holds,
    /// and the game repeats it, until the push falls back below half of it.
    pub menu_stick_push: f32,
    /// How long a lone stick click waits for the other before it acts, seconds: both clicks pressed
    /// within it are the recenter chord, and neither acts on its own.
    pub stick_chord_window_s: f32,
    /// How long both stick clicks must be held together to recenter the VR view, seconds.
    pub recenter_hold_s: f32,
    /// How soon after the left stick click a fresh right B press can show or hide the debug panel,
    /// seconds. The left click waits for the longer of this and the recenter join window.
    pub panel_chord_window_s: f32,
    /// Withhold the left-stick-first developer chords (the debug panel and the take marker) in a
    /// vehicle, so that the left stick click, a right-handed player's grapple there, waits only for
    /// the recenter join window rather than the panel's.
    pub suppress_developer_chords_in_vehicles: bool,
    /// The yank gesture: a sharp pull of the grapple hand toward the body zips along an attached
    /// line.
    pub yank: GrappleYankConfig,
}

impl BindingsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            move_deadzone: 0.15,
            trigger_deadzone: 0.05,
            long_press_s: 0.5,
            crouch_flick: 0.8,
            menu_stick_push: 0.5,
            stick_chord_window_s: 0.1,
            recenter_hold_s: 1.0,
            panel_chord_window_s: 0.35,
            suppress_developer_chords_in_vehicles: true,
            yank: GrappleYankConfig::new(),
        }
    }
}

impl Default for BindingsConfig {
    fn default() -> Self {
        Self::new()
    }
}
