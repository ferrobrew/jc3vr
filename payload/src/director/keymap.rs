//! The director's key bindings: a small config-driven map from key chords to actions, fed from
//! the window procedure so the bindings respect window focus and key repeat like the built-in
//! function keys do.
//!
//! The map is part of the runtime configuration ([`DirectorConfig::keymap`]), so it is saved in
//! the settings file with everything else, and rebinding is an edit to the `director.keymap`
//! section there (or the debug tab's reset). The defaults sit on the numeric keypad: nothing else
//! in the game or the mod uses it, and it is findable by touch under a headset. Controller chords
//! are not bound here yet; the in-headset debug panel covers the controller-only case for now.
//!
//! A saved map keeps its own list, so an action added after it was saved would have no key. An
//! action the saved map does not bind at all therefore takes its default chord, unless the saved
//! map binds that chord to something else ([`Keymap::effective`]).
//!
//! [`DirectorConfig::keymap`]: crate::director::DirectorConfig::keymap

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_ADD, VK_CONTROL, VK_DECIMAL, VK_DIVIDE, VK_MENU, VK_MULTIPLY, VK_NUMPAD0,
    VK_NUMPAD1, VK_NUMPAD2, VK_NUMPAD3, VK_NUMPAD4, VK_NUMPAD5, VK_NUMPAD6, VK_NUMPAD7, VK_NUMPAD8,
    VK_NUMPAD9, VK_SHIFT, VK_SUBTRACT,
};

use crate::{
    config::{self, Config},
    director::{heat, markers, spawn, teleport, teleport::TeleportMode, time},
};

/// Everything a key can trigger.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    ToggleCinematic,
    ToggleSlowMotion,
    TogglePause,
    TimeScaleUp,
    TimeScaleDown,
    TakeMarker,
    SaveBookmark,
    TeleportBookmark1,
    TeleportBookmark2,
    TeleportBookmark3,
    ToggleGodMode,
    ToggleHeatLock,
    ClearHeat,
    Spawn,
    ToggleRecording,
    ToggleTrackingRecording,
    ToggleReplay,
    ToggleScenario,
}
impl Action {
    /// A short label for the debug tab.
    pub fn label(self) -> &'static str {
        match self {
            Action::ToggleCinematic => "Toggle cinematic mode",
            Action::ToggleSlowMotion => "Toggle slow motion",
            Action::TogglePause => "Toggle pause",
            Action::TimeScaleUp => "Time scale up",
            Action::TimeScaleDown => "Time scale down",
            Action::TakeMarker => "Take marker",
            Action::SaveBookmark => "Save bookmark here",
            Action::TeleportBookmark1 => "Teleport to bookmark 1",
            Action::TeleportBookmark2 => "Teleport to bookmark 2",
            Action::TeleportBookmark3 => "Teleport to bookmark 3",
            Action::ToggleGodMode => "Toggle god mode",
            Action::ToggleHeatLock => "Toggle heat lock",
            Action::ClearHeat => "Clear heat",
            Action::Spawn => "Spawn the configured tags",
            Action::ToggleRecording => "Start or stop recording",
            Action::ToggleTrackingRecording => "Start or stop a tracking recording",
            Action::ToggleReplay => "Replay the last tracking recording, or stop the replay",
            Action::ToggleScenario => "Run the last scenario again, or stop it",
        }
    }
}

/// A key plus modifier requirements. `key` is a Win32 virtual-key code.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Chord {
    pub key: u16,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}
impl Chord {
    const fn plain(key: u16) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    const fn ctrl(key: u16) -> Self {
        Self {
            ctrl: true,
            ..Self::plain(key)
        }
    }

    /// A readable rendering for the debug tab.
    pub fn describe(self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.shift {
            parts.push("Shift".to_owned());
        }
        if self.alt {
            parts.push("Alt".to_owned());
        }
        parts.push(key_name(self.key));
        parts.join("+")
    }
}

/// One binding.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub action: Action,
    pub chord: Chord,
}

/// The whole map, as saved. The bindings borrow the stock layout until they are changed, so the
/// default map is a constant.
#[derive(Clone, Serialize, Deserialize)]
pub struct Keymap {
    pub bindings: Cow<'static, [Binding]>,
}
impl Keymap {
    /// The stock numeric-keypad layout.
    pub const fn new() -> Self {
        Self {
            bindings: Cow::Borrowed(&DEFAULT_BINDINGS),
        }
    }

    /// The bindings in force: the map's own, then the default binding of each action the map
    /// does not bind, unless the map binds its chord to something else.
    pub fn effective(&self) -> impl Iterator<Item = &Binding> {
        let defaults = DEFAULT_BINDINGS.iter().filter(|default| {
            !self
                .bindings
                .iter()
                .any(|b| b.action == default.action || b.chord == default.chord)
        });
        self.bindings.iter().chain(defaults)
    }

    /// The action bound to `key` under the live modifier state, if any. A chord with modifiers
    /// takes precedence over a plain binding of the same key.
    fn resolve(&self, key: u16, ctrl: bool, shift: bool, alt: bool) -> Option<Action> {
        self.effective()
            .filter(|b| {
                b.chord.key == key
                    && (!b.chord.ctrl || ctrl)
                    && (!b.chord.shift || shift)
                    && (!b.chord.alt || alt)
            })
            .max_by_key(|b| {
                u8::from(b.chord.ctrl) + u8::from(b.chord.shift) + u8::from(b.chord.alt)
            })
            .map(|b| b.action)
    }
}

/// The active map, for the debug tab.
pub fn current() -> Keymap {
    Config::lock_query(|c| c.director.keymap.clone())
}

/// Replace the map with the defaults.
pub fn reset_to_defaults() {
    config::edit().director.keymap = Keymap::new();
}

/// Handle a key-down from the window procedure. `first_press` is false on auto-repeat. Returns
/// whether a binding consumed the key. Runs on the window thread, which is the game's main thread.
pub fn on_key_down(key: u16, first_press: bool) -> bool {
    let (ctrl, shift, alt) = modifiers();
    let action = Config::lock_query(|c| c.director.keymap.resolve(key, ctrl, shift, alt));
    let Some(action) = action else {
        return false;
    };
    if first_press {
        run(action);
    }
    true
}

/// Fire an action, from a key or from the debug tab.
pub fn run(action: Action) {
    tracing::info!("director: {}", action.label());
    match action {
        Action::ToggleCinematic => {
            let mut cfg = config::edit();
            cfg.director.cinematic = !cfg.director.cinematic;
        }
        Action::ToggleSlowMotion => time::toggle_slow_motion(&mut config::edit().director),
        Action::TogglePause => {
            let mut cfg = config::edit();
            cfg.director.paused = !cfg.director.paused;
        }
        Action::TimeScaleUp => time::step_time_scale(&mut config::edit().director, 2.0),
        Action::TimeScaleDown => time::step_time_scale(&mut config::edit().director, 0.5),
        Action::TakeMarker => markers::mark(""),
        Action::SaveBookmark => {
            teleport::save_current("");
        }
        Action::TeleportBookmark1 => teleport::request_bookmark(0, TeleportMode::Auto),
        Action::TeleportBookmark2 => teleport::request_bookmark(1, TeleportMode::Auto),
        Action::TeleportBookmark3 => teleport::request_bookmark(2, TeleportMode::Auto),
        Action::ToggleGodMode => {
            let mut cfg = config::edit();
            cfg.director.god_mode = !cfg.director.god_mode;
        }
        Action::ToggleHeatLock => {
            let mut cfg = config::edit();
            cfg.director.heat_lock = !cfg.director.heat_lock;
        }
        Action::ClearHeat => heat::clear(),
        Action::Spawn => spawn::request(),
        Action::ToggleRecording => crate::recorder::toggle(),
        Action::ToggleTrackingRecording => crate::recorder::tracking::toggle(),
        Action::ToggleReplay => crate::testing::toggle_replay(),
        Action::ToggleScenario => crate::testing::toggle_scenario(),
    }
}

/// The stock layout [`Keymap::new`] borrows.
const DEFAULT_BINDINGS: [Binding; 18] = [
    Binding {
        action: Action::ToggleCinematic,
        chord: Chord::plain(VK_NUMPAD0.0),
    },
    Binding {
        action: Action::ToggleSlowMotion,
        chord: Chord::plain(VK_NUMPAD1.0),
    },
    Binding {
        action: Action::TogglePause,
        chord: Chord::plain(VK_NUMPAD2.0),
    },
    Binding {
        action: Action::TakeMarker,
        chord: Chord::plain(VK_NUMPAD3.0),
    },
    Binding {
        action: Action::TeleportBookmark1,
        chord: Chord::plain(VK_NUMPAD4.0),
    },
    Binding {
        action: Action::TeleportBookmark2,
        chord: Chord::plain(VK_NUMPAD5.0),
    },
    Binding {
        action: Action::TeleportBookmark3,
        chord: Chord::plain(VK_NUMPAD6.0),
    },
    Binding {
        action: Action::ToggleGodMode,
        chord: Chord::plain(VK_NUMPAD7.0),
    },
    Binding {
        action: Action::ClearHeat,
        chord: Chord::plain(VK_NUMPAD8.0),
    },
    Binding {
        action: Action::Spawn,
        chord: Chord::plain(VK_NUMPAD9.0),
    },
    Binding {
        action: Action::TimeScaleUp,
        chord: Chord::plain(VK_ADD.0),
    },
    Binding {
        action: Action::TimeScaleDown,
        chord: Chord::plain(VK_SUBTRACT.0),
    },
    Binding {
        action: Action::ToggleHeatLock,
        chord: Chord::plain(VK_MULTIPLY.0),
    },
    Binding {
        action: Action::SaveBookmark,
        chord: Chord::plain(VK_DECIMAL.0),
    },
    Binding {
        action: Action::ToggleRecording,
        chord: Chord::plain(VK_DIVIDE.0),
    },
    Binding {
        action: Action::ToggleTrackingRecording,
        chord: Chord::ctrl(VK_DIVIDE.0),
    },
    Binding {
        action: Action::ToggleReplay,
        chord: Chord::ctrl(VK_MULTIPLY.0),
    },
    Binding {
        action: Action::ToggleScenario,
        chord: Chord::ctrl(VK_SUBTRACT.0),
    },
];

/// The live modifier state at the time of a key message.
fn modifiers() -> (bool, bool, bool) {
    // SAFETY: `GetKeyState` reads the calling thread's message-queue key state; it is called from
    // the window procedure, whose thread owns that queue.
    let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
    (down(VK_CONTROL.0), down(VK_SHIFT.0), down(VK_MENU.0))
}

/// A name for a virtual-key code: the keypad and a few common keys by name, the rest as hex.
fn key_name(key: u16) -> String {
    match key {
        k if (VK_NUMPAD0.0..=VK_NUMPAD9.0).contains(&k) => format!("Numpad{}", k - VK_NUMPAD0.0),
        k if k == VK_ADD.0 => "Numpad+".to_owned(),
        k if k == VK_SUBTRACT.0 => "Numpad-".to_owned(),
        k if k == VK_MULTIPLY.0 => "Numpad*".to_owned(),
        k if k == VK_DECIMAL.0 => "Numpad.".to_owned(),
        k if k == VK_DIVIDE.0 => "Numpad/".to_owned(),
        k if (0x70..=0x87).contains(&k) => format!("F{}", k - 0x6F),
        k if k < 0x80
            && (char::from(k as u8).is_ascii_digit()
                || char::from(k as u8).is_ascii_uppercase()) =>
        {
            char::from(k as u8).to_string()
        }
        k => format!("VK 0x{k:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modified_chord_wins_over_the_plain_binding_of_the_same_key() {
        let map = Keymap {
            bindings: Cow::Owned(vec![
                Binding {
                    action: Action::TakeMarker,
                    chord: Chord::plain(VK_NUMPAD3.0),
                },
                Binding {
                    action: Action::ClearHeat,
                    chord: Chord {
                        key: VK_NUMPAD3.0,
                        ctrl: true,
                        shift: false,
                        alt: false,
                    },
                },
            ]),
        };
        assert_eq!(
            map.resolve(VK_NUMPAD3.0, false, false, false),
            Some(Action::TakeMarker)
        );
        assert_eq!(
            map.resolve(VK_NUMPAD3.0, true, false, false),
            Some(Action::ClearHeat)
        );
        // A key nothing binds, saved or by default.
        assert_eq!(map.resolve(u16::from(b'Q'), true, false, false), None);
    }

    #[test]
    fn an_action_the_saved_map_lacks_takes_its_default_unless_its_chord_is_taken() {
        let saved = |bindings: Vec<Binding>| Keymap {
            bindings: Cow::Owned(bindings),
        };
        // A map saved before the tracking recorder's binding existed.
        let old = saved(
            DEFAULT_BINDINGS
                .iter()
                .copied()
                .filter(|b| b.action != Action::ToggleTrackingRecording)
                .collect(),
        );
        assert_eq!(
            old.resolve(VK_DIVIDE.0, true, false, false),
            Some(Action::ToggleTrackingRecording)
        );
        // The same chord rebound to something else keeps its new action.
        let rebound = saved(vec![Binding {
            action: Action::TakeMarker,
            chord: Chord::ctrl(VK_DIVIDE.0),
        }]);
        assert_eq!(
            rebound.resolve(VK_DIVIDE.0, true, false, false),
            Some(Action::TakeMarker)
        );
        assert!(
            rebound
                .effective()
                .all(|b| b.action != Action::ToggleTrackingRecording)
        );
    }

    #[test]
    fn the_defaults_bind_every_action_once() {
        let map = Keymap::new();
        let mut chords: Vec<_> = map.bindings.iter().map(|b| b.chord).collect();
        chords.sort_by_key(|c| (c.key, c.ctrl, c.shift, c.alt));
        chords.dedup();
        assert_eq!(chords.len(), map.bindings.len());
    }
}
