//! Whether the player chose the VR entry: the VR entry follows the options menu's own entries, and
//! the movie lays the menu out on its column of main buttons, one entry per button in list order.
//! When the settings screen opens, the runtime reads every main button and this module decides
//! whether the selected one is the VR entry.

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

/// Whether the selected button is the VR entry, labelled `entry`.
///
/// The first visible selected button is the selection, as it is for the movie. Its label decides
/// when it can be read. When it cannot, the position decides: the VR entry is the last visible
/// button.
pub(crate) fn vr_entry_selected(buttons: &[MainButton], entry: &str) -> bool {
    let Some((position, button)) = buttons
        .iter()
        .enumerate()
        .find(|(_, button)| button.visible && button.selected)
    else {
        return false;
    };
    match &button.label {
        Some(label) => entry.eq_ignore_ascii_case(label.trim()),
        None => buttons
            .iter()
            .rposition(|button| button.visible)
            .is_some_and(|last| last == position),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str = "VR";

    /// Seven native entries, the VR entry, and three hidden buttons, with `selected` selected and
    /// every label readable.
    fn menu(selected: usize) -> Vec<MainButton> {
        let labels = [
            "GRAPHICS",
            "SOUND",
            "GAMEPLAY",
            "CONTROLLER",
            "KEYBOARD",
            "CREDITS",
            "DLC",
            ENTRY,
        ];
        let mut buttons: Vec<MainButton> = labels
            .into_iter()
            .map(|label| MainButton {
                visible: true,
                selected: false,
                label: Some(label.to_owned()),
            })
            .collect();
        buttons.extend((0..3).map(|_| MainButton {
            visible: false,
            selected: false,
            label: Some(String::new()),
        }));
        buttons[selected].selected = true;
        buttons
    }

    #[test]
    fn the_vr_entry_is_found_by_its_label() {
        assert!(vr_entry_selected(&menu(7), ENTRY));
    }

    #[test]
    fn a_native_entry_is_not_the_vr_entry() {
        assert!(!vr_entry_selected(&menu(0), ENTRY));
        assert!(!vr_entry_selected(&menu(6), ENTRY));
    }

    #[test]
    fn nothing_selected_is_not_the_vr_entry() {
        let mut buttons = menu(0);
        buttons[0].selected = false;
        assert!(!vr_entry_selected(&buttons, ENTRY));
    }

    #[test]
    fn a_hidden_button_left_selected_is_ignored() {
        let mut buttons = menu(7);
        buttons[9].selected = true;
        assert!(vr_entry_selected(&buttons, ENTRY));
        buttons[7].selected = false;
        assert!(!vr_entry_selected(&buttons, ENTRY));
    }

    #[test]
    fn labels_match_regardless_of_case_and_padding() {
        let mut buttons = menu(7);
        buttons[7].label = Some(" vr ".to_owned());
        assert!(vr_entry_selected(&buttons, ENTRY));
    }

    #[test]
    fn unreadable_labels_fall_back_to_the_position() {
        let mut buttons = menu(7);
        for button in &mut buttons {
            button.label = None;
        }
        assert!(vr_entry_selected(&buttons, ENTRY));
        buttons[7].selected = false;
        buttons[3].selected = true;
        assert!(!vr_entry_selected(&buttons, ENTRY));
    }
}
