//! Status text composed from authored prose and opaque values, with a separate legacy spelling.

use smol_str::SmolStr;

/// Text for the Status page and the existing log and scenario contracts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Text {
    pub(super) legacy: SmolStr,
    pub(super) display: SmolStr,
}

impl Text {
    pub(super) fn pair(legacy: impl Into<SmolStr>, display: impl Into<SmolStr>) -> Self {
        Self {
            legacy: legacy.into(),
            display: display.into(),
        }
    }

    pub(super) fn push(&mut self, part: impl Into<Self>) {
        let part = part.into();
        self.legacy = smol_str::format_smolstr!("{}{}", self.legacy, part.legacy);
        self.display = smol_str::format_smolstr!("{}{}", self.display, part.display);
    }

    pub(super) fn join(parts: impl IntoIterator<Item = Self>, separator: &str) -> Self {
        let mut joined = Self::default();
        for (index, part) in parts.into_iter().enumerate() {
            if index > 0 {
                joined.push(separator);
            }
            joined.push(part);
        }
        joined
    }

    pub(super) fn is_empty(&self) -> bool {
        self.legacy.is_empty()
    }
}

impl From<&str> for Text {
    fn from(value: &str) -> Self {
        let value = SmolStr::new(value);
        Self::pair(value.clone(), value)
    }
}

impl From<String> for Text {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl From<SmolStr> for Text {
    fn from(value: SmolStr) -> Self {
        Self::pair(value.clone(), value)
    }
}

/// Authored display prose whose uppercase spelling is retained in logs and scenario details.
pub(super) fn prose(display: impl AsRef<str>) -> Text {
    let display = display.as_ref();
    Text::pair(display.to_uppercase(), display)
}

/// Compose authored prose literals and opaque expressions without rewriting completed text.
macro_rules! text {
    (@parts $text:ident;) => {};
    (@parts $text:ident; ,) => {};
    (@parts $text:ident; $literal:literal, $($rest:tt)*) => {
        $text.push(crate::status::prose($literal));
        text!(@parts $text; $($rest)*);
    };
    (@parts $text:ident; $value:expr, $($rest:tt)*) => {
        $text.push($value);
        text!(@parts $text; $($rest)*);
    };
    ($($parts:tt)*) => {{
        let mut composed = crate::status::Text::default();
        text!(@parts composed; $($parts)*,);
        composed
    }};
}

pub(super) use text;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_keeps_prose_separate_from_opaque_values() {
        for value in [
            "123ABCDE",
            "DEADBEEF",
            "/FOO",
            "SLOT_ID",
            "MixedCaseName",
            "OpenXR",
            "P50/P95",
        ] {
            let text = text!(
                prose("Gun's shape not read: "),
                value,
                prose("; held+braced, game's grip")
            );
            assert_eq!(
                text.display,
                format!("Gun's shape not read: {value}; held+braced, game's grip")
            );
            assert_eq!(
                text.legacy,
                format!("GUN'S SHAPE NOT READ: {value}; HELD+BRACED, GAME'S GRIP")
            );
        }
    }

    #[test]
    fn nested_composition_and_notes_do_not_recase_values() {
        let shape = text!(
            prose("The slot's pistol: "),
            Text::from("C4"),
            ", ",
            Text::from("VR"),
            prose(" at "),
            Text::from("11.1 ms P50/P95")
        );
        let note = text!(
            prose("Another save's sidecar, "),
            shape,
            prose("; see "),
            Text::from("config/VRProfile.JSON")
        );
        assert_eq!(
            note.display,
            "Another save's sidecar, The slot's pistol: C4, VR at 11.1 ms P50/P95; see config/VRProfile.JSON"
        );
        assert_eq!(
            note.legacy,
            "ANOTHER SAVE'S SIDECAR, THE SLOT'S PISTOL: C4, VR AT 11.1 ms P50/P95; SEE config/VRProfile.JSON"
        );
    }
}
