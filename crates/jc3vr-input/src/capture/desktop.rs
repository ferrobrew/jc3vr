//! One sample of the physical desktop input (the keyboard, the mouse buttons, and the text
//! composition), as the capture policy consumes it. The sampler itself is native and lives with
//! the window (`egui_impl::desktop_input` in the HUD crate).

/// An unavailable sample never establishes desktop neutrality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopInputObservation {
    Unavailable(DesktopInputUnavailable),
    Observed(DesktopInputEvidence),
}

impl DesktopInputObservation {
    pub fn controls_neutral(self) -> Option<bool> {
        match self {
            Self::Unavailable(_) => None,
            Self::Observed(evidence) => Some(evidence.controls_neutral()),
        }
    }

    /// Native focus is known only for observed samples or an explicit focus failure.
    pub fn focused(self) -> Option<bool> {
        match self {
            Self::Observed(_) => Some(true),
            Self::Unavailable(DesktopInputUnavailable::Unfocused) => Some(false),
            Self::Unavailable(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopInputUnavailable {
    WindowNotObserved,
    WrongThread,
    Unfocused,
    KeyboardStateUnavailable,
    ImeStateUnavailable,
    BaselinePending,
    ConcurrentActivity,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DesktopModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub windows: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopInputEvidence {
    pub sequence: u64,
    pub held_virtual_keys: u16,
    pub held_mouse_buttons: u8,
    pub modifiers: DesktopModifiers,
    pub key_activity: bool,
    pub mouse_button_activity: bool,
    pub scroll_activity: bool,
    pub text_activity: bool,
    pub composition_active: bool,
    pub consumed_shortcuts: u16,
}

impl DesktopInputEvidence {
    pub fn controls_neutral(self) -> bool {
        self.held_virtual_keys == 0
            && self.held_mouse_buttons == 0
            && self.modifiers == DesktopModifiers::default()
            && !self.key_activity
            && !self.mouse_button_activity
            && !self.scroll_activity
            && !self.text_activity
            && !self.composition_active
            && self.consumed_shortcuts == 0
    }
}
