#![cfg_attr(any(), rustfmt::skip)]
/// `g_AltTab`: set by [`WndProc`](crate::window::WndProc) on `WM_CANCELMODE`. It forces [`AVAMainLoop`](crate::window::AVAMainLoop) to treat the next
/// iteration as a focus loss even when the foreground window did not change, and the loop clears
/// it after handling the change.
pub unsafe fn get_AltTab() -> &'static mut bool {
    unsafe { &mut *(0x142E22A14 as *mut bool) }
}
/// `g_SuspendGame`: whether the game is suspended because its window lost the foreground.
/// [`LoseFocus`](crate::window::LoseFocus) sets it, [`GainFocus`](crate::window::GainFocus) clears it, and `WM_DESTROY` in [`WndProc`](crate::window::WndProc) clears it.
/// [`AVAMainLoop`](crate::window::AVAMainLoop) skips [`Game::Update`](crate::game::Game::Update) while it is set during gameplay.
pub unsafe fn get_SuspendGame() -> &'static mut bool {
    unsafe { &mut *(0x142E22A11 as *mut bool) }
}
/// `g_WasFullscreen`: the fullscreen mode [`LoseFocus`](crate::window::LoseFocus) saved when it switched the window to
/// windowed mode, which [`GainFocus`](crate::window::GainFocus) restores. Initialized to the configured fullscreen mode at
/// startup.
pub unsafe fn get_WasFullscreen() -> &'static mut bool {
    unsafe { &mut *(0x142E22A12 as *mut bool) }
}
/// `gHWND`: the game's main window, which [`AVAMainLoop`](crate::window::AVAMainLoop) compares against the foreground window.
pub unsafe fn get_WindowHandle() -> &'static mut *mut ::std::ffi::c_void {
    unsafe { &mut *(0x142E22A18 as *mut *mut ::std::ffi::c_void) }
}
pub const WndProc_ADDRESS: usize = 0x140006F40;
/// The game's Win32 window procedure. It does not handle `WM_ACTIVATEAPP`, `WM_ACTIVATE`,
/// `WM_SETFOCUS`, or `WM_KILLFOCUS`: those fall through to `DefWindowProcA`, and the game tracks
/// window focus by polling the foreground window in [`AVAMainLoop`](crate::window::AVAMainLoop) instead. It sets
/// [`AltTab`](get_AltTab) on `WM_CANCELMODE`.
unsafe fn WndProc(
    hwnd: *mut ::std::ffi::c_void,
    msg: u32,
    wparam: u64,
    lparam: i64,
) -> i64 {
    unsafe {
        let f: unsafe extern "system" fn(
            hwnd: *mut ::std::ffi::c_void,
            msg: u32,
            wparam: u64,
            lparam: i64,
        ) -> i64 = ::std::mem::transmute(WndProc_ADDRESS);
        f(hwnd, msg, wparam, lparam)
    }
}
pub const AVAMainLoop_ADDRESS: usize = 0x14000B1A0;
/// `AVAMainLoop`: the application's main loop. Each iteration drains the thread's message queue
/// with `PeekMessageA` (dispatching to [`WndProc`](crate::window::WndProc) and quitting on `WM_QUIT`), then polls
/// `GetForegroundWindow`. When the foreground window differs from the one the previous iteration
/// saw, or [`AltTab`](get_AltTab) is set, it calls [`GainFocus`](crate::window::GainFocus) if the new foreground window is
/// [`WindowHandle`](get_WindowHandle) (and [`AltTab`](get_AltTab) is clear), and [`LoseFocus`](crate::window::LoseFocus) otherwise, then refreshes the
/// cursor clip ([`GraphicsEngine::UpdateCursor`](crate::graphics_engine::graphics_engine::GraphicsEngine::UpdateCursor))
/// and clears [`AltTab`](get_AltTab). Only a change of foreground window triggers either call, so a window
/// that never changes the foreground keeps the state it last set.
///
/// While [`SuspendGame`](get_SuspendGame) is set and the game state is
/// [`E_GAME_RUN`](crate::game::GameState::E_GAME_RUN), the iteration skips
/// [`Game::Update`](crate::game::Game::Update) and the `SleepEx` yield after it, so the loop spins on
/// the message queue and the foreground poll: no simulation, rendering, input, or present runs
/// until [`GainFocus`](crate::window::GainFocus) clears the flag. In any other game state (the frontend, loading, and so on)
/// the game keeps updating while suspended.
pub unsafe fn AVAMainLoop(
    instance: *mut ::std::ffi::c_void,
    prev_instance: *mut ::std::ffi::c_void,
    cmd_line: *mut u8,
) -> i32 {
    unsafe {
        let f: unsafe extern "system" fn(
            instance: *mut ::std::ffi::c_void,
            prev_instance: *mut ::std::ffi::c_void,
            cmd_line: *mut u8,
        ) -> i32 = ::std::mem::transmute(AVAMainLoop_ADDRESS);
        f(instance, prev_instance, cmd_line)
    }
}
pub const LoseFocus_ADDRESS: usize = 0x140001B70;
/// `LoseFocus`: suspends the game because its window lost the foreground. Called by
/// [`AVAMainLoop`](crate::window::AVAMainLoop) with the main window handle and whether any window holds the foreground. When
/// [`SuspendGame`](get_SuspendGame) is clear it sets it, tells the sound library the window lost focus
/// ([`SetWindowFocused`](crate::audio::sound_library::SetWindowFocused), which mutes the master bus),
/// broadcasts the `pause_game_checked` event, and clears the input device manager's
/// [`m_IsInFocus`](crate::input::input_device_manager::InputDeviceManager::m_IsInFocus). When
/// `allow_mode_change` is set and the graphics engine exists, it also saves the current
/// fullscreen mode in [`WasFullscreen`](get_WasFullscreen) and switches to windowed mode (through the settings
/// manager when one exists). It always refreshes the cursor. A call while already suspended only
/// refreshes the cursor.
pub unsafe fn LoseFocus(hwnd: *mut ::std::ffi::c_void, allow_mode_change: bool) {
    unsafe {
        let f: unsafe extern "system" fn(
            hwnd: *mut ::std::ffi::c_void,
            allow_mode_change: bool,
        ) = ::std::mem::transmute(LoseFocus_ADDRESS);
        f(hwnd, allow_mode_change)
    }
}
pub const GainFocus_ADDRESS: usize = 0x140001AB0;
/// `GainFocus`: resumes the game because its window regained the foreground. Called by
/// [`AVAMainLoop`](crate::window::AVAMainLoop) with the main window handle. When [`SuspendGame`](get_SuspendGame) is set it clears it, tells
/// the sound library the window has focus
/// ([`SetWindowFocused`](crate::audio::sound_library::SetWindowFocused)), sets the input device
/// manager's [`m_IsInFocus`](crate::input::input_device_manager::InputDeviceManager::m_IsInFocus),
/// restores the fullscreen mode saved in [`WasFullscreen`](get_WasFullscreen), clears [`WasFullscreen`](get_WasFullscreen), and
/// broadcasts the `pause_game_clear` event. When the game is not suspended it only applies the
/// fullscreen mode in [`WasFullscreen`](get_WasFullscreen), without clearing it. It always refreshes the cursor.
pub unsafe fn GainFocus(hwnd: *mut ::std::ffi::c_void) {
    unsafe {
        let f: unsafe extern "system" fn(hwnd: *mut ::std::ffi::c_void) = ::std::mem::transmute(
            GainFocus_ADDRESS,
        );
        f(hwnd)
    }
}
