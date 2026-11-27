use detours_macro::detour;
#[cfg(feature = "profiler")]
use jc3vr_core::profiler::capture;
use jc3vr_core::{clean_view, lifecycle};
use jc3vr_hud::{
    egui_impl::{EguiState, consume_desktop_shortcut, observe_desktop_message},
    hud::{cursor, egui_panel},
};
use jc3vr_render::screenshot;
use jc3vr_tooling::{director, recorder};
use jc3vr_view::{headpose, vr};
use re_utilities::hook_library::HookLibrary;
#[cfg(feature = "profiler")]
use windows::Win32::UI::Input::KeyboardAndMouse::VK_F9;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{
            VIRTUAL_KEY, VK_F1, VK_F2, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F10, VK_F12,
        },
        WindowsAndMessaging::{
            GetClientRect, SIZE_MINIMIZED, SIZE_RESTORED, WM_ACTIVATEAPP, WM_KEYDOWN, WM_KILLFOCUS,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN,
            WM_RBUTTONUP, WM_SETFOCUS, WM_SIZE, WM_SYSKEYDOWN,
        },
    },
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&WNDPROC_BINDER)
}

#[detour(address = jc3gi::window::WndProc_ADDRESS)]
fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    log_window_state(msg, wparam);
    if observe_desktop_message(msg, wparam, lparam) {
        return LRESULT(0);
    }
    if matches!(msg, WM_ACTIVATEAPP | WM_KILLFOCUS | WM_SETFOCUS) {
        return WNDPROC.get().unwrap().call(hwnd, msg, wparam, lparam);
    }
    // The fixed function keys, ahead of the director's keymap so they keep precedence. Consumed,
    // auto-repeat included, so neither egui nor the game acts on them.
    if matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN)
        && let Some(hotkey) = HOTKEYS
            .iter()
            .find(|hotkey| wparam.0 == hotkey.key.0 as usize)
    {
        consume_desktop_shortcut(wparam.0);
        if is_first_press(lparam) {
            (hotkey.press)();
        }
        return LRESULT(0);
    }

    // The director's bindable keys (`jc3vr_tooling::director::keymap`). Consumed when bound, so the
    // game does not also act on the key; auto-repeat is passed through as a non-first press and
    // ignored.
    if matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN)
        && director::keymap::on_key_down(wparam.0 as u16, is_first_press(lparam))
    {
        consume_desktop_shortcut(wparam.0);
        return LRESULT(0);
    }

    // Track the mouse buttons and wheel for the panel cursor (see `jc3vr_hud::hud::cursor`): the
    // `SendMouseEvents` detour hands Scaleform the live button bitmask each frame instead of the
    // steering action map's effector edges. Observed, not consumed -- egui and the game still see
    // the messages.
    match msg {
        WM_LBUTTONDOWN => cursor::on_button(cursor::Button::Left, true),
        WM_LBUTTONUP => cursor::on_button(cursor::Button::Left, false),
        WM_RBUTTONDOWN => cursor::on_button(cursor::Button::Right, true),
        WM_RBUTTONUP => cursor::on_button(cursor::Button::Right, false),
        // The wheel delta is the signed high word of `wParam`, in `WHEEL_DELTA` units.
        WM_MOUSEWHEEL => cursor::on_wheel(((wparam.0 >> 16) & 0xffff) as u16 as i16 as i32),
        _ => {}
    }
    // The mouse position (game-tracked from these same messages) is in window-client pixels, so
    // publish the live client rect as the cursor mapping's normalization space -- the back buffer
    // can be a different size (and aspect) than the window under Wine/Proton scaling.
    if msg == WM_MOUSEMOVE {
        let mut rect = RECT::default();
        // SAFETY: `hwnd` is the live game window handle this message arrived on.
        if unsafe { GetClientRect(hwnd, &mut rect) }.is_ok() {
            let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
            if w > 0 && h > 0 {
                cursor::set_client_size((w as u32, h as u32));
            }
        }
        // Publish the raw client-pixel mouse position for the VR panel pointer (issue #24), tracked
        // here so it stays fresh even while the panel has captured (disabled) the game's own input.
        let x = (lparam.0 & 0xffff) as i16 as i32;
        let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
        cursor::set_mouse_pos((x, y));
    }

    if let Some(egui_state) = EguiState::get().as_mut() {
        egui_state.wndproc(hwnd, msg, wparam, lparam);
    }
    WNDPROC.get().unwrap().call(hwnd, msg, wparam, lparam)
}

/// A fixed function key: what one press does. The key-down message's previous-state bit
/// ([`is_first_press`]) debounces each key on its own, so two keys pressed together both act.
struct Hotkey {
    key: VIRTUAL_KEY,
    press: fn(),
}

const HOTKEYS: &[Hotkey] = &[
    // A session recording: the video recorder and a tracking recording together, under one stamp.
    Hotkey {
        key: VK_F1,
        press: recorder::session_recording::toggle,
    },
    // A take marker into whatever records.
    Hotkey {
        key: VK_F2,
        press: recorder::session_recording::mark,
    },
    Hotkey {
        key: VK_F4,
        press: toggle_clean_view,
    },
    // Eject: the request only records the intent, and the game thread prepares and tears down.
    Hotkey {
        key: VK_F5,
        press: lifecycle::request_shutdown,
    },
    Hotkey {
        key: VK_F6,
        press: toggle_egui_capture,
    },
    Hotkey {
        key: VK_F7,
        press: recenter,
    },
    Hotkey {
        key: VK_F8,
        press: toggle_floating_panel,
    },
    #[cfg(feature = "profiler")]
    Hotkey {
        key: VK_F9,
        press: start_profiler_capture,
    },
    // Consuming F10 also keeps the system's menu activation off it.
    Hotkey {
        key: VK_F10,
        press: toggle_mirror_pair,
    },
    Hotkey {
        key: VK_F12,
        press: request_screenshot,
    },
];

/// Whether a key-down message is the first of its press rather than an auto-repeat: bit 30 of
/// `lParam` holds the key's previous state.
fn is_first_press(lparam: LPARAM) -> bool {
    lparam.0 & 0x4000_0000 == 0
}

/// Toggle the clean view at its scope (`jc3vr_core::clean_view`): the flat egui debug
/// overlay leaves the desktop window, and at the everything scope every other overlay goes too.
/// With no live VR panel, hiding releases editing and showing requests it through the device and
/// neutral gates. The consumed key episode remains blocked through its release.
fn toggle_clean_view() {
    let clean = clean_view::toggle();
    if let Some(egui_state) = EguiState::get().as_mut() {
        egui_state.desktop_visibility_changed(!clean);
    }
}

/// Toggle the flat desktop overlay's input capture.
fn toggle_egui_capture() {
    if let Some(egui_state) = EguiState::get().as_mut() {
        egui_state.toggle_game_input_capture();
    }
}

/// Snapshot the current headpose as the neutral reference, so later poses are relative to it.
fn recenter() {
    headpose::recenter();
    tracing::info!("Headpose recentered");
}

/// Summon or dismiss the interactive egui floating panel in VR (issue #24), together with egui's
/// input capture.
fn toggle_floating_panel() {
    let enabled = egui_panel::toggle();
    tracing::info!(
        "F8: egui floating panel {}",
        if enabled { "ON" } else { "OFF" }
    );
}

/// Start a profiler trace capture (issue #34): a few seconds of CPU and GPU frames, dumped to the
/// session's `profiles/` folder as Chrome trace-event JSON, so a capture can be taken in-headset
/// without the overlay. A no-op while one is already recording.
#[cfg(feature = "profiler")]
fn start_profiler_capture() {
    if capture::start(capture::DEFAULT_CAPTURE_SECS) {
        tracing::info!("F9: profiler trace capture started");
    }
}

/// Switch the desktop mirror between the stereo pair and the last single eye.
fn toggle_mirror_pair() {
    let view = vr::toggle_stereo_pair();
    tracing::info!("F10: desktop mirror view {view:?}");
}

/// Write a PNG of the linear back buffer to the session's `screenshots/` folder: under single-pass
/// collapse that is the full side-by-side render (both eyes), for in-headset diagnosis.
fn request_screenshot() {
    screenshot::request();
    tracing::info!("F12: screenshot requested");
}

/// Log the game window gaining or losing the foreground, and being minimized or restored. Outside a
/// VR session the engine's main loop skips `Game::Update` for as long as the window is in the
/// background (`window::AVAMainLoop`, `jc3vr_view::hooks::window_focus`), which the frame breakdown can
/// only report as untracked time, so these lines are what places a long frame against what the
/// window was doing. The engine itself ignores these messages and polls the foreground window.
fn log_window_state(msg: u32, wparam: WPARAM) {
    match msg {
        WM_ACTIVATEAPP => {
            let active = wparam.0 != 0;
            tracing::info!(target: "window", active, "window: the application activation changed");
        }
        WM_SIZE if wparam.0 == SIZE_MINIMIZED as usize => {
            tracing::info!(target: "window", "window: minimized");
        }
        WM_SIZE if wparam.0 == SIZE_RESTORED as usize => {
            tracing::debug!(target: "window", "window: restored");
        }
        _ => {}
    }
}
