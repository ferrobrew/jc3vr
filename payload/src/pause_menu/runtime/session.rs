//! The session: the menu state, the shown page, and the VR tabs' and headers' bookkeeping, shared
//! by the hooks.
//!
//! The lock is never held across a call into the movie or into an item's functions. Calls into
//! the movie can re-enter the command hook synchronously (a widget playing a sound sends an event
//! command), and the lock is not re-entrant; so each hook copies what it needs out, releases the
//! lock, does its work, and writes back. [`Session::generation`] tells a write-back whether the
//! session moved on in between.

use parking_lot::Mutex;

use crate::pause_menu::model::{
    headers::HeaderStyles,
    live::LiveSlots,
    page::Page,
    state::{MenuState, left_page},
    tabs::TabMemory,
};

/// The session state.
#[derive(Default)]
pub(crate) struct Session {
    pub(crate) state: MenuState,
    /// The shown VR page's contents as installed, for resolving the rows' commands.
    pub(crate) page: Option<Page>,
    /// What the live-value pass last wrote per row clip.
    pub(crate) live: LiveSlots,
    /// Set when the shown page must be rebuilt on the next update (a step changed, or the page
    /// was reset). Deferred rather than done in the command hook, because the movie is still inside
    /// its own handler there and redraws the selected row after the command returns.
    pub(crate) rebuild: bool,
    /// Raised by the command hook around the game's `populate_settings` handling, so the populate
    /// hook knows the screen is opening from an options entry rather than being refreshed.
    pub(crate) opening: bool,
    /// Bumped whenever the shown page is installed, replaced, or dropped.
    pub(crate) generation: u64,
    /// The VR tabs' memory for this pause: the last tab shown and each tab's position.
    pub(crate) tabs: TabMemory,
    /// Set when the VR entry opened the settings screen: the screen sets its tab bar up for the
    /// native pages after the page is installed, so the update after it replaces that with the VR
    /// tabs.
    pub(crate) setup_tabs: bool,
    /// A tab the player chose, switched to on the next update with the list in its root state.
    pub(crate) pending_tab: Option<usize>,
    /// The row the highlight was on at the last update, for the direction a header is skipped in.
    pub(crate) last_selected: Option<usize>,
    /// The row clips' label styles.
    pub(crate) styles: HeaderStyles,
    /// The address of the pause screen that added the VR entry, to tell its input calls from other
    /// screens'.
    pub(crate) screen: usize,
}

impl Session {
    /// Replace the shown page (or clear it) and start a new generation.
    pub(crate) fn set_page(&mut self, page: Option<Page>) {
        self.page = page;
        self.live.invalidate();
        self.rebuild = false;
        self.last_selected = None;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Move to `next`, dropping the recorded page when `next` no longer shows it. Returns the page
    /// the move left ([`left_page`]), whose close work the caller runs once the lock is released.
    pub(crate) fn transition(&mut self, next: MenuState) -> Option<usize> {
        let left = left_page(self.state, next);
        self.state = next;
        if next == MenuState::Closed {
            self.opening = false;
            self.tabs = TabMemory::new();
            self.styles.forget();
            self.screen = 0;
        }
        if next.shown_page().is_none() {
            self.setup_tabs = false;
            self.pending_tab = None;
        }
        if left.is_some() || next.shown_page().is_none() {
            self.set_page(None);
        }
        left
    }

    /// Record `page` as the shown page after a rebuild of page `page_index` that started at
    /// `generation`, unless the session moved on meanwhile (the page was left, closed, or replaced
    /// from another path). Returns whether it was recorded.
    pub(crate) fn rebuilt(&mut self, generation: u64, page_index: usize, page: Page) -> bool {
        if self.generation != generation || self.state.shown_page() != Some(page_index) {
            return false;
        }
        self.set_page(Some(page));
        true
    }

    /// Whether a requested rebuild runs now. While the list is not in its root state (an option
    /// picker or message box is open) the request stays, and the rebuild runs on a later frame.
    pub(crate) fn take_rebuild(&mut self, list_in_root_state: bool) -> bool {
        if self.rebuild && list_in_root_state {
            self.rebuild = false;
            true
        } else {
            false
        }
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
    tabs: TabMemory::new(),
    setup_tabs: false,
    pending_tab: None,
    last_selected: None,
    styles: HeaderStyles::new(),
    screen: 0,
});

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWN: MenuState = MenuState::Shown { pages: 3, page: 1 };

    fn shown_session() -> Session {
        let mut session = Session {
            state: SHOWN,
            ..Session::default()
        };
        session.set_page(Some(Page::new("PAGE")));
        session
    }

    #[test]
    fn closing_from_any_path_leaves_the_page_once() {
        let mut session = shown_session();
        session.opening = true;
        assert_eq!(session.transition(MenuState::Closed), Some(1));
        assert!(session.page.is_none());
        assert!(!session.opening);
        // The unload after the deactivation, or the next activation, finds nothing to leave.
        assert_eq!(session.transition(MenuState::Closed), None);
    }

    #[test]
    fn leaving_the_screen_keeps_the_entries() {
        let mut session = shown_session();
        assert_eq!(session.transition(SHOWN.left_settings()), Some(1));
        assert_eq!(session.state, MenuState::EntryAdded { pages: 3 });
        assert!(session.page.is_none());
        assert_eq!(session.transition(MenuState::Closed), None);
    }

    #[test]
    fn closing_forgets_the_tabs_and_the_clip_styles() {
        let mut session = shown_session();
        session.tabs.shown(2);
        session.styles.changed(4, true);
        session.setup_tabs = true;
        session.pending_tab = Some(0);
        session.screen = 0x1000;
        session.transition(SHOWN.left_settings());
        // Leaving the screen keeps the memory for the next visit, and drops the pending tab work.
        assert_eq!(session.tabs.last(), 2);
        assert_eq!(session.styles.styled().count(), 1);
        assert!(!session.setup_tabs);
        assert_eq!(session.pending_tab, None);
        session.transition(MenuState::Closed);
        assert_eq!(session.tabs.last(), 0);
        assert_eq!(session.styles.styled().count(), 0);
        assert_eq!(session.screen, 0);
    }

    #[test]
    fn switching_tabs_leaves_the_old_page() {
        let mut session = shown_session();
        session.last_selected = Some(3);
        assert_eq!(session.transition(SHOWN.switch_tab(2)), Some(1));
        assert!(session.page.is_none());
        assert_eq!(session.last_selected, None);
        assert_eq!(session.state.shown_page(), Some(2));
    }

    #[test]
    fn staying_keeps_the_page() {
        let mut session = shown_session();
        let generation = session.generation;
        assert_eq!(session.transition(SHOWN), None);
        assert!(session.page.is_some());
        assert_eq!(session.generation, generation);
    }

    #[test]
    fn a_rebuild_is_recorded_only_if_the_session_did_not_move_on() {
        let mut session = shown_session();
        let generation = session.generation;
        assert!(session.rebuilt(generation, 1, Page::new("NEW")));
        assert_eq!(session.page.as_ref().unwrap().title, "NEW");

        let generation = session.generation;
        session.transition(MenuState::Closed);
        assert!(!session.rebuilt(generation, 1, Page::new("LATE")));
        assert!(session.page.is_none());

        let mut session = shown_session();
        assert!(!session.rebuilt(session.generation, 0, Page::new("OTHER")));
        let stale = session.generation;
        session.set_page(Some(Page::new("REPLACED")));
        assert!(!session.rebuilt(stale, 1, Page::new("LATE")));
        assert_eq!(session.page.as_ref().unwrap().title, "REPLACED");
    }

    #[test]
    fn a_rebuild_waits_for_the_list_to_return_to_its_root_state() {
        let mut session = shown_session();
        assert!(!session.take_rebuild(true));
        session.rebuild = true;
        assert!(!session.take_rebuild(false));
        assert!(session.rebuild);
        assert!(session.take_rebuild(true));
        assert!(!session.rebuild);
    }
}
