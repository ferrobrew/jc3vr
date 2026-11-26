//! The game window's Win32 geometry and title.
//!
//! The HWND is reached through the graphics params rather than being cached, because the engine owns
//! the window and the mod has no creation hook to latch onto. Every read goes to `GetClientRect`, so
//! a WM-driven resize is picked up on the next frame without any notification plumbing.

use jc3gi::graphics_engine::graphics_engine::get_graphics_params;
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{GetClientRect, InternalGetWindowText},
};

/// The game window's client size (`width`, `height`) in pixels, or `None` if it cannot be read or is
/// degenerate.
pub(crate) fn client_size() -> Option<(u32, u32)> {
    let (left, top, right, bottom) = rect()?;
    let (w, h) = (right - left, bottom - top);
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

/// The game window's title, or `None` if it cannot be read or is empty. Read with
/// `InternalGetWindowText`, which copies the stored text without sending the window a message, so
/// it cannot wait on the window's thread whatever thread calls it.
pub(crate) fn title() -> Option<String> {
    let hwnd: HWND = unsafe { get_graphics_params() }.m_Hwnd;
    let mut buffer = [0u16; 256];
    // SAFETY: `hwnd` is the live game window handle; the call writes at most `buffer.len()` units.
    let len = unsafe { InternalGetWindowText(hwnd, &mut buffer) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
    Some(String::from_utf16_lossy(&buffer[..len]))
}

/// The game window's client rect (`left, top, right, bottom`), or `None` if it cannot be read.
pub(super) fn rect() -> Option<(i32, i32, i32, i32)> {
    let hwnd: HWND = unsafe { get_graphics_params() }.m_Hwnd;
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is the live game window handle; `GetClientRect` errors on a bad handle.
    unsafe { GetClientRect(hwnd, &mut rect) }.ok()?;
    Some((rect.left, rect.top, rect.right, rect.bottom))
}
