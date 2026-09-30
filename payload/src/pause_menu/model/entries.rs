//! Which VR entry the player chose: the VR entries follow the options menu's own entries, and the
//! movie lays the menu out on its column of main buttons, one entry per button in list order. When
//! the settings screen opens, the runtime reads every main button and this module decides which VR
//! page, if any, the selected one stands for.

/// What the runtime read from one main button.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct MainButton {
    /// Whether the button shows an entry. The movie hides the buttons past the menu's last entry
    /// and leaves their other state as it was, so a hidden button can still report itself
    /// selected. `true` when the flag could not be read.
    pub(crate) visible: bool,
    /// Whether the button reports itself selected.
    pub(crate) selected: bool,
    /// The button's label, when it could be read.
    pub(crate) label: Option<String>,
}

/// The VR page (an index into `entries`, the VR entries' labels in the order they were added) that
/// the selected button stands for, or `None` when a native entry is selected or none is.
///
/// The first visible selected button is the selection, as it is for the movie. Its label decides
/// when it can be read: a VR entry's label names its page, and any other label is a native entry.
/// When the label cannot be read, the position decides: the VR entries are the last
/// `entries.len()` visible buttons.
pub(crate) fn selected_page(buttons: &[MainButton], entries: &[&str]) -> Option<usize> {
    let (position, button) = buttons
        .iter()
        .enumerate()
        .find(|(_, button)| button.visible && button.selected)?;
    match &button.label {
        Some(label) => entries
            .iter()
            .position(|entry| entry.eq_ignore_ascii_case(label.trim())),
        None => {
            let visible = buttons.iter().filter(|button| button.visible).count();
            let first = visible.checked_sub(entries.len())?;
            position
                .checked_sub(first)
                .filter(|&page| page < entries.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRIES: [&str; 3] = ["VR: BODY", "VR: CONTROLS", "VR: COMFORT"];

    /// Seven native entries, the three VR entries, and two hidden buttons, with `selected`
    /// selected and every label readable.
    fn menu(selected: usize) -> Vec<MainButton> {
        let labels = [
            "GRAPHICS",
            "SOUND",
            "GAMEPLAY",
            "CONTROLLER",
            "KEYBOARD",
            "CREDITS",
            "DLC",
        ]
        .into_iter()
        .chain(ENTRIES);
        let mut buttons: Vec<MainButton> = labels
            .map(|label| MainButton {
                visible: true,
                selected: false,
                label: Some(label.to_owned()),
            })
            .collect();
        buttons.extend((0..2).map(|_| MainButton {
            visible: false,
            selected: false,
            label: Some(String::new()),
        }));
        buttons[selected].selected = true;
        buttons
    }

    #[test]
    fn a_vr_entry_is_found_by_its_label() {
        assert_eq!(selected_page(&menu(7), &ENTRIES), Some(0));
        assert_eq!(selected_page(&menu(9), &ENTRIES), Some(2));
    }

    #[test]
    fn a_native_entry_is_not_a_page() {
        assert_eq!(selected_page(&menu(0), &ENTRIES), None);
        assert_eq!(selected_page(&menu(6), &ENTRIES), None);
    }

    #[test]
    fn nothing_selected_is_no_page() {
        let mut buttons = menu(0);
        buttons[0].selected = false;
        assert_eq!(selected_page(&buttons, &ENTRIES), None);
    }

    #[test]
    fn a_hidden_button_left_selected_is_ignored() {
        let mut buttons = menu(8);
        buttons[11].selected = true;
        assert_eq!(selected_page(&buttons, &ENTRIES), Some(1));
        buttons[8].selected = false;
        assert_eq!(selected_page(&buttons, &ENTRIES), None);
    }

    #[test]
    fn labels_match_regardless_of_case_and_padding() {
        let mut buttons = menu(8);
        buttons[8].label = Some(" vr: controls ".to_owned());
        assert_eq!(selected_page(&buttons, &ENTRIES), Some(1));
    }

    #[test]
    fn unreadable_labels_fall_back_to_the_position() {
        let mut buttons = menu(8);
        for button in &mut buttons {
            button.label = None;
        }
        assert_eq!(selected_page(&buttons, &ENTRIES), Some(1));
        buttons[8].selected = false;
        buttons[3].selected = true;
        assert_eq!(selected_page(&buttons, &ENTRIES), None);
    }

    #[test]
    fn more_entries_than_visible_buttons_find_nothing_by_position() {
        let buttons = vec![MainButton {
            visible: true,
            selected: true,
            label: None,
        }];
        assert_eq!(selected_page(&buttons, &ENTRIES), None);
    }
}
