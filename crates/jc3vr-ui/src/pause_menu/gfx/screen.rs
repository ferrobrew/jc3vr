//! A screen reached through its raw pointer.

use std::ptr::{NonNull, addr_of};

use jc3gi::ui::{overlay_ui::UIBaseState, ui_base::UIBase};

use crate::pause_menu::gfx::{CaptureThread, GfxValue};

/// A live screen, held as a pointer rather than a reference.
///
/// A call into the screen's movie can re-enter the screen's own command handler, which writes the
/// screen's fields through its own pointer. A `&mut` to the screen held across such a call would
/// alias those writes, so the hooks keep the pointer and each access reads a field through a raw
/// place, or borrows the screen only for the duration of one engine call that does not re-enter
/// the movie.
pub(crate) struct Screen {
    ui: NonNull<UIBase>,
}

impl Screen {
    /// The screen at `ui`, or `None` for a null pointer.
    ///
    /// # Safety
    /// `ui` must be a live screen for as long as the returned value is used; a detour's own `this`
    /// is, for the duration of the detour.
    pub(crate) unsafe fn new(ui: *mut UIBase) -> Option<Self> {
        NonNull::new(ui).map(|ui| Self { ui })
    }

    /// Whether the screen is [`ACTIVE`](UIBaseState::ACTIVE).
    pub(crate) fn is_active(&self) -> bool {
        // SAFETY: a live screen (`new`); the state is read as its underlying integer, so a value
        // outside the enum cannot be observed as one.
        let state = unsafe { addr_of!((*self.ui.as_ptr()).m_Current).cast::<i32>().read() };
        state == UIBaseState::ACTIVE as i32
    }

    /// A new reference to the screen's clip, or `None` when the screen has none (it is not
    /// loaded).
    pub(crate) fn clip<'t>(&self, thread: &'t CaptureThread) -> Option<GfxValue<'t>> {
        // SAFETY: `m_Clip` is the screen's live clip value while the screen is loaded; the shared
        // borrow lasts only for the copy, whose `ObjectAddRef` does not enter the movie. The copy
        // takes its own reference, released on drop.
        let clip = unsafe { GfxValue::copy_of(thread, &*addr_of!((*self.ui.as_ptr()).m_Clip)) };
        clip.is_object().then_some(clip)
    }

    /// Run `f` with the screen borrowed mutably, for one engine call that does not re-enter the
    /// movie's command handler (creating an object or an array).
    pub(super) fn with_ui<R>(&self, f: impl FnOnce(&mut UIBase) -> R) -> R {
        // SAFETY: a live screen (`new`); the borrow ends with `f`, and callers pass only calls that
        // do not reach the screen's own code.
        f(unsafe { &mut *self.ui.as_ptr() })
    }
}
