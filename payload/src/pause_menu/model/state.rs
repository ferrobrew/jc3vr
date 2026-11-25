//! The VR menu's state across one pause-menu session: whether the VR entry was added to the root
//! menu, and which VR tab, if any, the settings screen shows.
//!
//! `Closed` → (the entry added on activation) `EntryAdded` → (the VR entry opens the settings
//! screen) `Shown` → (another tab) `Shown` → (the player leaves the screen) `EntryAdded` → (the
//! menu deactivates) `Closed`. The runtime drives the transitions from the hooks; this module only
//! decides them, including which page a transition leaves ([`left_page`]), whose close work is then
//! due. Switching tabs leaves the old tab's page like any other way out of it.

/// The menu state.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub(crate) enum MenuState {
    /// No VR entry in the loaded movie (the menu is closed, the feature is off, or adding the entry
    /// failed).
    #[default]
    Closed,
    /// The root menu has the VR entry, whose settings screen has `pages` tabs, one per registered
    /// page in registry order.
    EntryAdded { pages: usize },
    /// The settings screen shows the VR tabs, with page `page` selected.
    Shown { pages: usize, page: usize },
}

/// Why the settings pages are being populated.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum PopulateSource {
    /// The movie is opening the settings screen from a menu entry: the VR entry when `vr` is set,
    /// a native entry otherwise.
    Opened { vr: bool },
    /// The game repopulated the pages of a screen already shown (its refresh flag).
    Refresh,
}

impl MenuState {
    /// The state after adding the VR entry, with `pages` tabs, to a freshly loaded movie.
    pub(crate) fn entry_added(pages: usize) -> Self {
        if pages == 0 {
            MenuState::Closed
        } else {
            MenuState::EntryAdded { pages }
        }
    }

    /// The state after the settings pages were populated, and the VR page to install over the
    /// native page the screen shows, if any. The VR entry opens on tab `tab` (the last one shown
    /// this session), or on the first when `tab` is out of range.
    pub(crate) fn populate(self, source: PopulateSource, tab: usize) -> (Self, Option<usize>) {
        let pages = match self {
            MenuState::Closed => return (self, None),
            MenuState::EntryAdded { pages } | MenuState::Shown { pages, .. } => pages,
        };
        match (self, source) {
            (_, PopulateSource::Opened { vr: true }) => {
                let page = if tab < pages { tab } else { 0 };
                (MenuState::Shown { pages, page }, Some(page))
            }
            // A native entry opened the screen: its pages stay as the game built them.
            (_, PopulateSource::Opened { vr: false }) => (MenuState::EntryAdded { pages }, None),
            (MenuState::Shown { page, .. }, PopulateSource::Refresh) => (self, Some(page)),
            (_, PopulateSource::Refresh) => (self, None),
        }
    }

    /// The state after the player chose tab `page`. Only a shown screen switches, and only to a
    /// tab that exists.
    pub(crate) fn switch_tab(self, page: usize) -> Self {
        match self {
            MenuState::Shown { pages, .. } if page < pages => MenuState::Shown { pages, page },
            other => other,
        }
    }

    /// The state after the player left the settings screen.
    pub(crate) fn left_settings(self) -> Self {
        match self {
            MenuState::Shown { pages, .. } => MenuState::EntryAdded { pages },
            other => other,
        }
    }

    /// The state after installing a VR page failed: the screen shows whatever the game put there.
    pub(crate) fn install_failed(self) -> Self {
        self.left_settings()
    }

    /// The state after the menu deactivated or its movie unloaded: the entry is gone with the
    /// movie, or about to be.
    pub(crate) fn closed(self) -> Self {
        MenuState::Closed
    }

    /// The VR page shown, if any.
    pub(crate) fn shown_page(self) -> Option<usize> {
        match self {
            MenuState::Shown { page, .. } => Some(page),
            _ => None,
        }
    }
}

/// The VR page the transition from `before` to `after` leaves, if any: the page `before` showed,
/// when `after` no longer shows it. A page's close work runs exactly once per stay, whichever path
/// ends it (a tab switch included), because a second transition out of a state that shows no page,
/// or that still shows the same page, leaves nothing.
pub(crate) fn left_page(before: MenuState, after: MenuState) -> Option<usize> {
    before
        .shown_page()
        .filter(|&page| after.shown_page() != Some(page))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENED_VR: PopulateSource = PopulateSource::Opened { vr: true };
    const OPENED_NATIVE: PopulateSource = PopulateSource::Opened { vr: false };

    #[test]
    fn a_full_session() {
        let state = MenuState::entry_added(3);
        assert_eq!(state, MenuState::EntryAdded { pages: 3 });

        let (state, install) = state.populate(OPENED_VR, 0);
        assert_eq!(state, MenuState::Shown { pages: 3, page: 0 });
        assert_eq!(install, Some(0));

        let state = state.switch_tab(2);
        assert_eq!(state, MenuState::Shown { pages: 3, page: 2 });

        let (state, install) = state.populate(PopulateSource::Refresh, 0);
        assert_eq!(state.shown_page(), Some(2));
        assert_eq!(install, Some(2));

        let state = state.left_settings();
        assert_eq!(state, MenuState::EntryAdded { pages: 3 });

        let (state, install) = state.populate(OPENED_NATIVE, 2);
        assert_eq!(state, MenuState::EntryAdded { pages: 3 });
        assert_eq!(install, None);

        // Reopening the VR entry returns to the last tab.
        let (state, install) = state.populate(OPENED_VR, 2);
        assert_eq!(state, MenuState::Shown { pages: 3, page: 2 });
        assert_eq!(install, Some(2));
    }

    #[test]
    fn no_pages_means_closed() {
        assert_eq!(MenuState::entry_added(0), MenuState::Closed);
    }

    #[test]
    fn closed_ignores_everything() {
        for source in [OPENED_VR, OPENED_NATIVE, PopulateSource::Refresh] {
            assert_eq!(
                MenuState::Closed.populate(source, 0),
                (MenuState::Closed, None)
            );
        }
        assert_eq!(MenuState::Closed.left_settings(), MenuState::Closed);
        assert_eq!(MenuState::Closed.switch_tab(0), MenuState::Closed);
    }

    #[test]
    fn a_remembered_tab_past_the_pages_opens_the_first() {
        let state = MenuState::entry_added(2);
        assert_eq!(
            state.populate(OPENED_VR, 5),
            (MenuState::Shown { pages: 2, page: 0 }, Some(0))
        );
    }

    #[test]
    fn tab_switches_need_a_shown_screen_and_an_existing_tab() {
        let entry = MenuState::entry_added(3);
        assert_eq!(entry.switch_tab(1), entry);
        let shown = MenuState::Shown { pages: 3, page: 0 };
        assert_eq!(shown.switch_tab(3), shown);
        assert_eq!(shown.switch_tab(1), MenuState::Shown { pages: 3, page: 1 });
    }

    #[test]
    fn a_refresh_without_a_vr_page_installs_nothing() {
        let state = MenuState::entry_added(2);
        assert_eq!(state.populate(PopulateSource::Refresh, 0), (state, None));
    }

    #[test]
    fn opening_a_native_entry_from_a_shown_page_leaves_it() {
        let shown = MenuState::Shown { pages: 3, page: 0 };
        assert_eq!(
            shown.populate(OPENED_NATIVE, 0),
            (MenuState::EntryAdded { pages: 3 }, None)
        );
    }

    #[test]
    fn a_failed_install_leaves_the_page() {
        let shown = MenuState::Shown { pages: 1, page: 0 };
        assert_eq!(shown.install_failed(), MenuState::EntryAdded { pages: 1 });
    }

    #[test]
    fn every_way_out_of_a_page_leaves_it_once() {
        let shown = MenuState::Shown { pages: 3, page: 1 };
        for after in [
            shown.left_settings(),
            shown.install_failed(),
            shown.closed(),
            shown.switch_tab(2),
            shown.populate(OPENED_NATIVE, 1).0,
        ] {
            assert_eq!(left_page(shown, after), Some(1), "{after:?}");
            // A second close, from whichever path comes next, does not leave the page again.
            assert_ne!(left_page(after, after.closed()), Some(1), "{after:?}");
        }
    }

    #[test]
    fn staying_on_a_page_leaves_nothing() {
        let shown = MenuState::Shown { pages: 3, page: 1 };
        assert_eq!(left_page(shown, shown), None);
        assert_eq!(left_page(shown, shown.switch_tab(1)), None);
        assert_eq!(
            left_page(shown, shown.populate(PopulateSource::Refresh, 0).0),
            None
        );
        let entry = MenuState::entry_added(3);
        assert_eq!(left_page(entry, entry.closed()), None);
        assert_eq!(left_page(MenuState::Closed, MenuState::Closed), None);
    }
}
