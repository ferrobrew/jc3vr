//! The registry of VR pages: one options-menu entry per page, in this order at the top of the
//! options menu. A page's definition lives beside the settings it edits; register it here.

mod demo;

use crate::pause_menu::model::page::PageDef;

/// The registered pages.
pub(crate) const PAGES: &[PageDef] = &[demo::PAGE];

/// The most pages the options menu can take: the movie lays a menu out on a fixed number of main
/// buttons ([`PauseUI::MAIN_BUTTON_COUNT`](jc3gi::ui::pause_ui::PauseUI::MAIN_BUTTON_COUNT)) and
/// drops the entries past the last, and the options menu's own entries fill all but this many on
/// PC. The VR entries go first, so a page past this limit would push a native entry off the menu.
pub(crate) const MAX_PAGES: usize = 4;

#[cfg(test)]
mod tests {
    use jc3gi::ui::pause_ui::PauseUI;

    use super::*;

    #[test]
    fn the_registry_fits_the_options_menu() {
        assert!(!PAGES.is_empty());
        assert!(PAGES.len() <= MAX_PAGES);
        assert!(MAX_PAGES < PauseUI::MAIN_BUTTON_COUNT as usize);
    }

    #[test]
    fn entry_labels_are_plain_text() {
        // The movie's translator treats a label beginning with `&` or `#` as a localisation key.
        for page in PAGES {
            assert!(!page.entry.is_empty());
            assert!(
                !page.entry.starts_with(['&', '#']),
                "{:?} would be translated",
                page.entry
            );
        }
    }

    #[test]
    fn entry_labels_are_distinct() {
        // The selected entry is confirmed by its label, so two pages must not share one.
        for (i, a) in PAGES.iter().enumerate() {
            for b in &PAGES[i + 1..] {
                assert_ne!(a.entry, b.entry);
            }
        }
    }
}
