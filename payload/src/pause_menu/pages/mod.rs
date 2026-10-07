//! The registry of VR pages: one tab per page under the options menu's VR entry, in this order. A
//! page's definition lives beside the settings it edits; register it here.

use crate::{
    controllers::{body, menu},
    pause_menu::model::{page::PageDef, tabs::TAB_LIMIT},
};

/// The VR entry's label in the options menu. Plain text: the movie treats a label beginning with
/// `&` or `#` as a localisation key.
pub(crate) const ENTRY_LABEL: &str = "VR";

/// The registered pages.
pub(crate) const PAGES: &[PageDef] = &[
    body::menu::PAGE,
    menu::controls::PAGE,
    menu::comfort::PAGE,
    menu::display::PAGE,
    menu::status::PAGE,
];

/// The most pages the VR entry can show: one per selector on the settings screen's tab bar. A page
/// past this limit would have no tab.
pub(crate) const MAX_PAGES: usize = TAB_LIMIT;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pause_menu::model::{page::Item, rows::encode_page};

    #[test]
    fn the_registry_fits_the_tab_bar() {
        assert!(!PAGES.is_empty());
        assert!(PAGES.len() <= MAX_PAGES);
    }

    #[test]
    fn labels_are_plain_text() {
        // The movie's translator treats a label beginning with `&` or `#` as a localisation key.
        for label in std::iter::once(ENTRY_LABEL).chain(PAGES.iter().map(|page| page.tab)) {
            assert!(!label.is_empty());
            assert!(
                !label.starts_with(['&', '#']),
                "{label:?} would be translated"
            );
            assert_eq!(label, label.to_uppercase());
        }
    }

    #[test]
    fn every_page_builds_and_encodes() {
        for (index, page) in PAGES.iter().enumerate() {
            let built = (page.build)();
            assert!(!built.items.is_empty(), "{}", page.tab);
            assert_eq!(encode_page(index, &built).unwrap().len(), built.items.len());
        }
    }

    #[test]
    fn slider_notches_step_evenly_from_the_keyboard() {
        // The movie's left and right move a slider by a tenth of its notches, truncated to an
        // integer: fewer than ten notches would not move at all, and a count that is not a
        // multiple of ten would step unevenly.
        for page in PAGES {
            for item in (page.build)().items {
                if let Item::Slider(slider) = item {
                    let steps = slider.range.max_notch();
                    assert!(steps >= 10 && steps % 10 == 0, "{}", slider.label);
                }
            }
        }
    }

    #[test]
    fn labels_are_written_as_the_movie_shows_them() {
        // The movie upper-cases row labels but shows option labels as given; writing both in upper
        // case keeps the source what the player sees.
        for page in PAGES {
            let built = (page.build)();
            assert_eq!(built.title, built.title.to_uppercase(), "{}", page.tab);
            for item in &built.items {
                let (label, options): (&str, &[&str]) = match item {
                    Item::Header(text) | Item::Text(text) => (text, &[]),
                    Item::Toggle(toggle) => (&toggle.label, &[]),
                    Item::Choice(choice) => (&choice.label, choice.options),
                    Item::Slider(slider) => (&slider.label, &[]),
                    Item::Readout(readout) => (&readout.label, &[]),
                    Item::Button(button) => (&button.label, &[]),
                };
                for text in std::iter::once(label).chain(options.iter().copied()) {
                    assert_eq!(text, text.to_uppercase(), "{}", page.tab);
                }
            }
        }
    }

    #[test]
    fn tab_labels_are_distinct() {
        for (i, a) in PAGES.iter().enumerate() {
            for b in &PAGES[i + 1..] {
                assert_ne!(a.tab, b.tab);
            }
        }
    }
}
