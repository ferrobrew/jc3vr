//! The session: the menu state and the shown page, shared by the hooks.
//!
//! The lock is never held across a call into the movie or into an item's functions. Calls into
//! the movie can re-enter the command hook synchronously (a widget playing a sound sends an event
//! command), and the lock is not re-entrant; so each hook copies what it needs out, releases the
//! lock, does its work, and writes back. [`Session::generation`] tells a write-back whether the
//! session moved on in between.

use parking_lot::Mutex;

use crate::pause_menu::model::{live::LiveSlots, page::Page, state::MenuState};

/// The session state.
#[derive(Default)]
pub(crate) struct Session {
    pub(crate) state: MenuState,
    /// The shown VR page's contents as installed, for resolving the rows' commands.
    pub(crate) page: Option<Page>,
    /// What the live-value pass last wrote per row clip.
    pub(crate) live: LiveSlots,
    /// Set when the shown page must be rebuilt on the next update (a flow step changed, or the page
    /// was reset). Deferred rather than done in the command hook, because the movie is still inside
    /// its own handler there and redraws the selected row after the command returns.
    pub(crate) rebuild: bool,
    /// Raised by the command hook around the game's `populate_settings` handling, so the populate
    /// hook knows the screen is opening from an options entry rather than being refreshed.
    pub(crate) opening: bool,
    /// Bumped whenever the shown page is installed, replaced, or dropped.
    pub(crate) generation: u64,
}

impl Session {
    /// Replace the shown page (or clear it) and start a new generation.
    pub(crate) fn set_page(&mut self, page: Option<Page>) {
        self.page = page;
        self.live.invalidate();
        self.rebuild = false;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Return to the closed state, dropping the page.
    pub(crate) fn close(&mut self) {
        self.state = MenuState::Closed;
        self.opening = false;
        self.set_page(None);
    }
}

/// The session, shared by the pause-menu hooks (all on the capture thread).
pub(crate) static SESSION: Mutex<Session> = Mutex::new(Session {
    state: MenuState::Closed,
    page: None,
    live: LiveSlots::new(),
    rebuild: false,
    opening: false,
    generation: 0,
});
