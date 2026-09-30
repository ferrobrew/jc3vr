//! The VR menu's state across one pause-menu session: whether the VR entries were added to the
//! options menu, and which VR page, if any, the settings screen shows.
//!
//! `Closed` → (entries added on activation) `EntriesAdded` → (a VR entry opens the settings
//! screen) `Shown` → (the player leaves the screen) `EntriesAdded` → (the menu deactivates)
//! `Closed`. The runtime drives the transitions from the hooks; this module only decides them.

/// The menu state.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub(crate) enum MenuState {
    /// No VR entries in the loaded movie (the menu is closed, the feature is off, or adding the
    /// entries failed).
    #[default]
    Closed,
    /// The first `count` options-menu entries are the VR pages, in registry order.
    EntriesAdded { count: usize },
    /// The settings screen shows VR page `page`.
    Shown { count: usize, page: usize },
}

/// Why the settings pages are being populated.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum PopulateSource {
    /// The movie is opening the settings screen from an options-menu entry. `selected` is the
    /// options-menu position of the entry the player chose, when one reports itself selected.
    Opened { selected: Option<usize> },
    /// The game repopulated the pages of a screen already shown (its refresh flag).
    Refresh,
}

impl MenuState {
    /// The state after adding `count` VR entries to a freshly loaded movie.
    pub(crate) fn entries_added(count: usize) -> Self {
        if count == 0 {
            MenuState::Closed
        } else {
            MenuState::EntriesAdded { count }
        }
    }

    /// The state after the settings pages were populated, and the VR page to install over the
    /// native page the screen shows, if any.
    pub(crate) fn populate(self, source: PopulateSource) -> (Self, Option<usize>) {
        let count = match self {
            MenuState::Closed => return (self, None),
            MenuState::EntriesAdded { count } | MenuState::Shown { count, .. } => count,
        };
        match (self, source) {
            (_, PopulateSource::Opened { selected }) => match selected {
                Some(page) if page < count => (MenuState::Shown { count, page }, Some(page)),
                // A native entry opened the screen: its pages stay as the game built them.
                _ => (MenuState::EntriesAdded { count }, None),
            },
            (MenuState::Shown { page, .. }, PopulateSource::Refresh) => (self, Some(page)),
            (_, PopulateSource::Refresh) => (self, None),
        }
    }

    /// The state after the player left the settings screen.
    pub(crate) fn left_settings(self) -> Self {
        match self {
            MenuState::Shown { count, .. } => MenuState::EntriesAdded { count },
            other => other,
        }
    }

    /// The state after installing a VR page failed: the screen shows whatever the game put there.
    pub(crate) fn install_failed(self) -> Self {
        self.left_settings()
    }

    /// The VR page shown, if any.
    pub(crate) fn shown_page(self) -> Option<usize> {
        match self {
            MenuState::Shown { page, .. } => Some(page),
            _ => None,
        }
    }

    /// How many VR entries lead the options menu.
    pub(crate) fn entry_count(self) -> usize {
        match self {
            MenuState::Closed => 0,
            MenuState::EntriesAdded { count } | MenuState::Shown { count, .. } => count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENED_NATIVE: PopulateSource = PopulateSource::Opened { selected: None };

    fn opened(selected: usize) -> PopulateSource {
        PopulateSource::Opened {
            selected: Some(selected),
        }
    }

    #[test]
    fn a_full_session() {
        let state = MenuState::entries_added(3);
        assert_eq!(state, MenuState::EntriesAdded { count: 3 });

        let (state, install) = state.populate(opened(1));
        assert_eq!(state, MenuState::Shown { count: 3, page: 1 });
        assert_eq!(install, Some(1));

        let (state, install) = state.populate(PopulateSource::Refresh);
        assert_eq!(state.shown_page(), Some(1));
        assert_eq!(install, Some(1));

        let state = state.left_settings();
        assert_eq!(state, MenuState::EntriesAdded { count: 3 });

        let (state, install) = state.populate(OPENED_NATIVE);
        assert_eq!(state, MenuState::EntriesAdded { count: 3 });
        assert_eq!(install, None);
    }

    #[test]
    fn no_entries_means_closed() {
        assert_eq!(MenuState::entries_added(0), MenuState::Closed);
        assert_eq!(MenuState::Closed.entry_count(), 0);
    }

    #[test]
    fn closed_ignores_everything() {
        for source in [opened(0), OPENED_NATIVE, PopulateSource::Refresh] {
            assert_eq!(
                MenuState::Closed.populate(source),
                (MenuState::Closed, None)
            );
        }
        assert_eq!(MenuState::Closed.left_settings(), MenuState::Closed);
    }

    #[test]
    fn a_selection_past_the_entries_is_native() {
        let state = MenuState::entries_added(2);
        assert_eq!(state.populate(opened(2)), (state, None));
    }

    #[test]
    fn a_refresh_without_a_vr_page_installs_nothing() {
        let state = MenuState::entries_added(2);
        assert_eq!(state.populate(PopulateSource::Refresh), (state, None));
    }

    #[test]
    fn opening_again_from_a_shown_page_follows_the_selection() {
        let shown = MenuState::Shown { count: 3, page: 0 };
        assert_eq!(
            shown.populate(opened(2)),
            (MenuState::Shown { count: 3, page: 2 }, Some(2))
        );
        assert_eq!(
            shown.populate(OPENED_NATIVE),
            (MenuState::EntriesAdded { count: 3 }, None)
        );
    }

    #[test]
    fn a_failed_install_leaves_the_page() {
        let shown = MenuState::Shown { count: 1, page: 0 };
        assert_eq!(shown.install_failed(), MenuState::EntriesAdded { count: 1 });
        assert_eq!(shown.entry_count(), 1);
    }
}
