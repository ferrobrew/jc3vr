//! The gamepad-semantics pin: while the controller scheme is in effect, the game's "the gamepad is
//! the active input device" setting
//! ([`SettingType::EPCUseGamepad`](jc3gi::settings_manager::SettingType::EPCUseGamepad)) is held at
//! the gamepad's value.
//!
//! The game sets the value from the frame's device events. The controllers inject at the action
//! level and produce no device events, so without the pin the value stays at whatever device the
//! player last touched, and the game applies that device's rules: aim assist only on the gamepad,
//! a click-style melee on the keyboard, and the matching button prompts. The controller bindings
//! are written against the gamepad rules, so the pin holds the gamepad value.
//!
//! The entry is written directly rather than through the setter, which would also re-select the
//! controller port and may persist the change. The value is restored when the pin ends, unless the
//! game has since written its own value from a real device event.

use jc3gi::settings_manager::{SettingType, SettingsManager};
use parking_lot::Mutex;

use crate::{config::Config, controllers::scheme};

/// The value the pin holds: the gamepad is the active input device.
pub const GAMEPAD: i32 = 1;

/// The pin's readout for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SettingStatus {
    /// Whether the pin held the setting on the last tick.
    pub pinned: bool,
    /// The value the setting had on the last tick, before any write, or `None` before the settings
    /// manager exists.
    pub observed: Option<i32>,
    /// The value that will be restored when the pin ends.
    pub saved: Option<i32>,
}

/// The pin's state across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SettingPin {
    saved: Option<i32>,
}

impl SettingPin {
    pub const fn new() -> Self {
        Self { saved: None }
    }

    /// Advance one tick with the setting's current value `current`: returns the value to write, if
    /// any.
    ///
    /// While `active`, the pin remembers the value to restore and holds [`GAMEPAD`]. A value other
    /// than [`GAMEPAD`] while pinned can only be the game's own write from a keyboard or mouse event,
    /// so it becomes the value to restore. Once inactive, the remembered value is restored if the
    /// setting still holds the pinned value.
    pub fn step(&mut self, active: bool, current: i32) -> Option<i32> {
        if active {
            if self.saved.is_none() || current != GAMEPAD {
                self.saved = Some(current);
            }
            return (current != GAMEPAD).then_some(GAMEPAD);
        }
        let saved = self.saved.take()?;
        (current == GAMEPAD && saved != GAMEPAD).then_some(saved)
    }

    /// The value to restore when the pin ends.
    pub fn saved(&self) -> Option<i32> {
        self.saved
    }
}

/// The latest readout.
pub fn status() -> SettingStatus {
    STATE.lock().status
}

/// Hold or restore the setting for this input tick. Called from the engine's input tick on the
/// main thread.
pub(crate) fn input_tick() {
    let config = Config::lock_query(|c| c.controllers);
    let active = config.enabled && config.gamepad_semantics && scheme::is_controllers();
    let Some(manager) = (unsafe { SettingsManager::get() }) else {
        let mut state = STATE.lock();
        state.pin = SettingPin::new();
        state.status = SettingStatus::default();
        return;
    };
    let entry = &mut manager.m_Settings[SettingType::EPCUseGamepad as usize];
    let current = *entry;
    let mut state = STATE.lock();
    if let Some(value) = state.pin.step(active, current) {
        *entry = value;
    }
    state.status = SettingStatus {
        pinned: active,
        observed: Some(current),
        saved: state.pin.saved(),
    };
}

struct State {
    pin: SettingPin,
    status: SettingStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    pin: SettingPin::new(),
    status: SettingStatus {
        pinned: false,
        observed: None,
        saved: None,
    },
});

#[cfg(test)]
mod tests {
    use super::*;

    const KEYBOARD: i32 = 0;
    const UNSET: i32 = -1;

    #[test]
    fn the_pin_holds_the_gamepad_and_restores_the_keyboard() {
        let mut pin = SettingPin::new();
        assert_eq!(pin.step(true, KEYBOARD), Some(GAMEPAD));
        assert_eq!(pin.step(true, GAMEPAD), None);
        assert_eq!(pin.step(false, GAMEPAD), Some(KEYBOARD));
        assert_eq!(pin.step(false, KEYBOARD), None);
    }

    #[test]
    fn a_real_keyboard_event_while_pinned_becomes_the_value_to_restore() {
        let mut pin = SettingPin::new();
        assert_eq!(pin.step(true, UNSET), Some(GAMEPAD));
        assert_eq!(pin.step(true, KEYBOARD), Some(GAMEPAD));
        assert_eq!(pin.saved(), Some(KEYBOARD));
    }

    #[test]
    fn a_gamepad_value_is_left_alone() {
        let mut pin = SettingPin::new();
        assert_eq!(pin.step(true, GAMEPAD), None);
        assert_eq!(pin.step(false, GAMEPAD), None);
    }

    #[test]
    fn the_game_changing_the_value_after_the_pin_is_not_overwritten() {
        let mut pin = SettingPin::new();
        pin.step(true, KEYBOARD);
        assert_eq!(pin.step(false, UNSET), None);
        assert_eq!(pin.saved(), None);
    }

    #[test]
    fn an_idle_pin_writes_nothing() {
        let mut pin = SettingPin::new();
        assert_eq!(pin.step(false, KEYBOARD), None);
        assert_eq!(pin.step(false, GAMEPAD), None);
    }
}
