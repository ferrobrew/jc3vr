//! The VR tabs: which tab the VR entry opens on, each tab's scroll position and selected row, and
//! the commands the tab bar sends.
//!
//! The settings screen keeps a scroll memory per tab of its own, but writes it only through its
//! own tab switch, keyed by the page slot, which the native pages share; a VR tab switch through it
//! would leave a VR page's position for a native page opened later in the same pause. The VR tabs
//! therefore never go through it: every VR tab is shown in the same slot, the switch is the mod's,
//! and this module keeps the VR tabs' positions apart from the screen's.
//!
//! The tab bar reports a choice by calling its select callback with the tab's index. The mod sets
//! the callback to the pause document's `Invoke`, which sends the index, as text, as a command
//! name; [`TabCommands`] maps those command hashes back to indices.

use jc3gi::ui::pause_ui::SettingsUI;

/// The most tabs the tab bar holds ([`SettingsUI::TAB_COUNT`]).
pub(crate) const TAB_LIMIT: usize = SettingsUI::TAB_COUNT as usize;

/// A list's scroll position and selected row, as the list manager reports and restores them: the
/// index of the first row shown and the index of the selected row, both in the page's rows.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct ScrollState {
    pub(crate) top: usize,
    pub(crate) selected: usize,
}

impl ScrollState {
    /// The state as the list reads it back, or `None` when either index is negative.
    pub(crate) fn from_list(top: i32, selected: i32) -> Option<Self> {
        Some(Self {
            top: usize::try_from(top).ok()?,
            selected: usize::try_from(selected).ok()?,
        })
    }

    /// This state fitted to a page of `rows` rows shown `visible` at a time: the selected row is a
    /// row of the page, the top leaves no empty clips below the last row, and the selected row is
    /// on screen. A page whose rows changed since the state was saved (a step-by-step page, or a
    /// reset) would otherwise restore a position past its end.
    pub(crate) fn fitted(self, rows: usize, visible: usize) -> Self {
        if rows == 0 || visible == 0 {
            return Self::default();
        }
        let selected = self.selected.min(rows - 1);
        let max_top = rows.saturating_sub(visible);
        let lowest_top_showing_selected = (selected + 1).saturating_sub(visible);
        let top = self
            .top
            .min(max_top)
            .min(selected)
            .max(lowest_top_showing_selected);
        Self { top, selected }
    }
}

/// The VR tabs' memory for one pause session.
#[derive(Clone, Debug, Default)]
pub(crate) struct TabMemory {
    /// The tab the VR entry opens on: the last one shown.
    last: usize,
    /// Each tab's position when it was last left, by page index.
    saved: Vec<Option<ScrollState>>,
}

impl TabMemory {
    /// An empty memory: the first tab, at the top.
    pub(crate) const fn new() -> Self {
        Self {
            last: 0,
            saved: Vec::new(),
        }
    }

    /// The tab the VR entry opens on.
    pub(crate) fn last(&self) -> usize {
        self.last
    }

    /// Record that tab `page` is shown.
    pub(crate) fn shown(&mut self, page: usize) {
        self.last = page;
    }

    /// Record tab `page`'s position as it is left.
    pub(crate) fn save(&mut self, page: usize, state: ScrollState) {
        if self.saved.len() <= page {
            self.saved.resize(page + 1, None);
        }
        self.saved[page] = Some(state);
    }

    /// The position to restore tab `page` to, fitted to its `rows` rows shown `visible` at a time;
    /// `None` when the tab has not been left this session (it opens at its top).
    pub(crate) fn restore(&self, page: usize, rows: usize, visible: usize) -> Option<ScrollState> {
        self.saved
            .get(page)
            .copied()
            .flatten()
            .map(|state| state.fitted(rows, visible))
    }
}

/// The command hashes the tab bar's callback sends, one per tab index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TabCommands {
    hashes: [u32; TAB_LIMIT],
}

impl TabCommands {
    /// The commands, hashing each tab index's command name with `hash` (the engine's command hash,
    /// [`tab_command_name`] of the index).
    pub(crate) fn new(hash: impl Fn(&str) -> u32) -> Self {
        Self {
            hashes: std::array::from_fn(|index| hash(&tab_command_name(index))),
        }
    }

    /// The tab a command hash selects, if it is a tab command.
    pub(crate) fn tab_of(&self, command: u32) -> Option<usize> {
        self.hashes.iter().position(|&hash| hash == command)
    }
}

/// The command name the tab bar's callback sends for tab `index`: the document's `Invoke` takes
/// the callback's integer argument as the command name, as text.
pub(crate) fn tab_command_name(index: usize) -> String {
    index.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_list_indices_are_rejected() {
        assert_eq!(
            ScrollState::from_list(2, 5),
            Some(ScrollState {
                top: 2,
                selected: 5
            })
        );
        assert_eq!(ScrollState::from_list(-1, 5), None);
        assert_eq!(ScrollState::from_list(0, -1), None);
    }

    #[test]
    fn a_state_that_fits_is_kept() {
        let state = ScrollState {
            top: 4,
            selected: 9,
        };
        assert_eq!(state.fitted(30, 13), state);
    }

    #[test]
    fn a_shorter_page_clamps_the_selection_and_the_top() {
        let state = ScrollState {
            top: 20,
            selected: 28,
        };
        assert_eq!(
            state.fitted(10, 13),
            ScrollState {
                top: 0,
                selected: 9
            }
        );
        assert_eq!(
            state.fitted(25, 13),
            ScrollState {
                top: 12,
                selected: 24
            }
        );
    }

    #[test]
    fn the_selected_row_stays_on_screen() {
        // Selected above the window.
        assert_eq!(
            ScrollState {
                top: 10,
                selected: 3
            }
            .fitted(40, 13),
            ScrollState {
                top: 3,
                selected: 3
            }
        );
        // Selected below the window.
        assert_eq!(
            ScrollState {
                top: 0,
                selected: 20
            }
            .fitted(40, 13),
            ScrollState {
                top: 8,
                selected: 20
            }
        );
    }

    #[test]
    fn an_empty_page_restores_the_top() {
        let state = ScrollState {
            top: 3,
            selected: 7,
        };
        assert_eq!(state.fitted(0, 13), ScrollState::default());
        assert_eq!(state.fitted(5, 0), ScrollState::default());
    }

    #[test]
    fn every_fitted_state_is_valid() {
        for rows in 1..40 {
            for visible in 1..15 {
                for top in 0..45 {
                    for selected in 0..45 {
                        let fitted = ScrollState { top, selected }.fitted(rows, visible);
                        assert!(fitted.selected < rows);
                        assert!(fitted.top <= fitted.selected);
                        assert!(fitted.selected < fitted.top + visible);
                        assert!(fitted.top <= rows.saturating_sub(visible));
                    }
                }
            }
        }
    }

    #[test]
    fn the_memory_is_per_tab_and_remembers_the_last() {
        let mut memory = TabMemory::new();
        assert_eq!(memory.last(), 0);
        assert_eq!(memory.restore(1, 20, 13), None);

        let state = ScrollState {
            top: 2,
            selected: 6,
        };
        memory.save(1, state);
        memory.shown(2);
        assert_eq!(memory.last(), 2);
        assert_eq!(memory.restore(1, 20, 13), Some(state));
        assert_eq!(memory.restore(0, 20, 13), None);
        assert_eq!(memory.restore(2, 20, 13), None);
        // A later save replaces the earlier one.
        memory.save(1, ScrollState::default());
        assert_eq!(memory.restore(1, 20, 13), Some(ScrollState::default()));
        // A restore is fitted to the page as it is now.
        memory.save(0, state);
        assert_eq!(
            memory.restore(0, 4, 13),
            Some(ScrollState {
                top: 0,
                selected: 3
            })
        );
    }

    #[test]
    fn tab_commands_map_back_to_their_index() {
        // A stand-in hash; the runtime uses the engine's.
        let commands = TabCommands::new(|name| 1000 + name.parse::<u32>().unwrap());
        for index in 0..TAB_LIMIT {
            assert_eq!(commands.tab_of(1000 + index as u32), Some(index));
        }
        assert_eq!(commands.tab_of(999), None);
        assert_eq!(commands.tab_of(1000 + TAB_LIMIT as u32), None);
    }

    #[test]
    fn command_names_are_the_index_as_text() {
        assert_eq!(tab_command_name(0), "0");
        assert_eq!(tab_command_name(8), "8");
    }
}
