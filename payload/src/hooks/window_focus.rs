//! Keeps the engine running while the desktop window is in the background during a VR session.
//!
//! The engine suspends itself whenever its window loses the foreground: `window::AVAMainLoop` polls
//! the foreground window and calls `window::LoseFocus`, which sets `window::SuspendGame`, mutes the
//! audio, drops keyboard and mouse input, and leaves fullscreen; while that flag is set during
//! gameplay the loop stops calling `Game::Update` at all, so the VR frame loop, the head pose, and
//! the controllers freeze until the window is focused again. In VR the desktop window's focus says
//! nothing about where the player's attention is, and a window manager, an overlay, or a terminal
//! takes it often, so while a session runs these detours skip the suspension.
//!
//! Only the keyboard and mouse still follow the desktop focus: the engine reads the mouse through a
//! background DirectInput device, so leaving `InputDeviceManager::m_IsInFocus` set would let mouse
//! motion and clicks meant for another window drive the game. Gamepads and the OpenXR controllers
//! do not consult it. See `docs/mod/window-focus.md`.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::{
    input::input_device_manager::InputDeviceManager,
    window::{get_SuspendGame, get_WindowHandle},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::GetForegroundWindow};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&LOSE_FOCUS_BINDER)
        .with_static_binder(&GAIN_FOCUS_BINDER)
}

/// Lift a suspension the engine applied before the VR session started running. The engine keeps
/// updating while suspended outside gameplay (the frontend, a load), so a focus loss on the main
/// menu, followed by the headset session starting, would otherwise freeze the game the moment the
/// load reaches gameplay. Runs the engine's own resume (`window::GainFocus`), which restores what
/// `window::LoseFocus` changed, then puts the keyboard and mouse focus back in line with the
/// desktop. Call on the game thread at the top of `Game::Update`.
pub(crate) fn resume_for_vr() {
    // SAFETY: the main loop and `Game::Update` share the game thread, the only writer of the flag.
    if !crate::vr::is_running_published() || !unsafe { *get_SuspendGame() } {
        return;
    }
    tracing::info!(
        target: "window",
        "window: resuming the game the engine suspended on a focus loss, as a VR session runs"
    );
    // SAFETY: the engine's main window handle, written once at window creation.
    let hwnd = unsafe { *get_WindowHandle() };
    GAIN_FOCUS.get().unwrap().call(hwnd);
    let foreground = is_foreground(hwnd);
    set_input_focus(foreground);
    PAIRING.lock().resumed(foreground);
}

/// The pairing of skipped focus losses with their gains. Game thread only.
static PAIRING: Mutex<FocusPairing> = Mutex::new(FocusPairing::new());

#[detour(address = jc3gi::window::LoseFocus_ADDRESS)]
fn lose_focus(hwnd: *mut c_void, allow_mode_change: bool) {
    // SAFETY: called by the main loop on the game thread, the only writer of the flag.
    let suspended = unsafe { *get_SuspendGame() };
    let decision = PAIRING
        .lock()
        .lose(crate::vr::is_running_published(), suspended);
    match decision {
        Decision::Skip { first } => {
            if first {
                tracing::info!(
                    target: "window",
                    "window: the window lost the foreground; the game keeps running while a VR \
                     session runs"
                );
            }
            set_input_focus(false);
        }
        Decision::Forward => LOSE_FOCUS.get().unwrap().call(hwnd, allow_mode_change),
    }
}

#[detour(address = jc3gi::window::GainFocus_ADDRESS)]
fn gain_focus(hwnd: *mut c_void) {
    // SAFETY: called by the main loop on the game thread, the only writer of the flag.
    let suspended = unsafe { *get_SuspendGame() };
    let decision = PAIRING.lock().gain(suspended);
    match decision {
        Decision::Skip { .. } => set_input_focus(true),
        Decision::Forward => GAIN_FOCUS.get().unwrap().call(hwnd),
    }
}

/// Whether `hwnd` holds the desktop foreground.
fn is_foreground(hwnd: *mut c_void) -> bool {
    // SAFETY: `GetForegroundWindow` takes no arguments and only reads window-manager state.
    let foreground = unsafe { GetForegroundWindow() };
    foreground == HWND(hwnd)
}

/// Set the input device manager's view of the window focus, which gates the keyboard and mouse.
fn set_input_focus(in_focus: bool) {
    // SAFETY: the input device manager singleton is read and written on the game thread only.
    if let Some(manager) = unsafe { InputDeviceManager::get() } {
        manager.m_IsInFocus = in_focus;
    }
}

/// What a focus detour does with the engine's call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// Skip the engine's function, only mirroring the desktop focus into the keyboard and mouse.
    /// `first` is set on the first skipped loss of a background stretch, for logging.
    Skip { first: bool },
    /// Run the engine's function.
    Forward,
}

/// Pairs each skipped `window::LoseFocus` with the `window::GainFocus` that ends it. The engine's
/// resume, run on a game that is not suspended, still applies `window::WasFullscreen`, which holds
/// whatever the last real resume left there, so the gain that matches a skipped loss is skipped
/// too, whether or not the session still runs by then.
#[derive(Debug)]
struct FocusPairing {
    /// Whether a loss was skipped and its gain has not come yet.
    loss_skipped: bool,
}

impl FocusPairing {
    const fn new() -> Self {
        Self {
            loss_skipped: false,
        }
    }

    /// Decide a `window::LoseFocus` call: skipped while a VR session runs, unless the engine is
    /// already suspended, in which case the call is the engine's own no-op and goes through.
    fn lose(&mut self, vr_running: bool, suspended: bool) -> Decision {
        if vr_running && !suspended {
            let first = !self.loss_skipped;
            self.loss_skipped = true;
            Decision::Skip { first }
        } else {
            Decision::Forward
        }
    }

    /// Decide a `window::GainFocus` call: skipped when it ends a skipped loss. A gain on a
    /// suspended engine always goes through, since it is the engine's resume.
    fn gain(&mut self, suspended: bool) -> Decision {
        if !suspended && std::mem::take(&mut self.loss_skipped) {
            Decision::Skip { first: false }
        } else {
            Decision::Forward
        }
    }

    /// Record that [`resume_for_vr`] ran the engine's resume. While the window is still in the
    /// background the main loop remembers another window as the foreground, so its next
    /// `window::GainFocus`, when the window comes forward, ends a loss the resume already undid.
    fn resumed(&mut self, foreground: bool) {
        self.loss_skipped = !foreground;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loss_during_a_session_is_skipped_and_its_gain_with_it() {
        let mut pairing = FocusPairing::new();
        assert_eq!(pairing.lose(true, false), Decision::Skip { first: true });
        assert_eq!(pairing.lose(true, false), Decision::Skip { first: false });
        assert_eq!(pairing.gain(false), Decision::Skip { first: false });
        assert_eq!(pairing.gain(false), Decision::Forward);
    }

    #[test]
    fn the_gain_of_a_skipped_loss_is_skipped_after_the_session_ends() {
        let mut pairing = FocusPairing::new();
        assert_eq!(pairing.lose(true, false), Decision::Skip { first: true });
        assert_eq!(pairing.gain(false), Decision::Skip { first: false });
        assert_eq!(pairing.lose(false, false), Decision::Forward);
        assert_eq!(pairing.gain(true), Decision::Forward);
    }

    #[test]
    fn without_a_session_every_call_goes_through() {
        let mut pairing = FocusPairing::new();
        assert_eq!(pairing.lose(false, false), Decision::Forward);
        assert_eq!(pairing.gain(true), Decision::Forward);
        assert_eq!(pairing.gain(false), Decision::Forward);
    }

    #[test]
    fn a_suspended_engine_keeps_its_own_loss_and_resume() {
        let mut pairing = FocusPairing::new();
        assert_eq!(pairing.lose(true, true), Decision::Forward);
        assert_eq!(pairing.gain(true), Decision::Forward);
    }

    #[test]
    fn a_resume_in_the_background_pairs_with_the_next_gain() {
        let mut pairing = FocusPairing::new();
        pairing.resumed(false);
        assert_eq!(pairing.gain(false), Decision::Skip { first: false });

        pairing.resumed(true);
        assert_eq!(pairing.gain(false), Decision::Forward);
    }
}
