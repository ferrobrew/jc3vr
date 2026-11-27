//! Resolving the summary's strings: the game's through its localisation store
//! ([`jc3vr_body::controllers::intent::localise`]), the mod's from [`ModText`].
//!
//! In a language the panel's fonts cannot draw ([`renders`]) every game string falls back to its
//! English stand-in, the key's own name made readable ([`humanise`]), as does a key the game does
//! not have.

use jc3gi::ui::locator::Locale;
use jc3vr_body::controllers::intent::localise::renders;

use crate::controllers::summary::labels::{ModText, Text};

/// Resolve `text` in `locale`, looking the game's keys up with `game`.
pub fn resolve(
    text: Text,
    locale: Option<Locale>,
    game: impl Fn(&str) -> Option<String>,
) -> String {
    match text {
        Text::Game(key) => locale
            .filter(|&l| renders(l))
            .and_then(|_| game(key))
            .unwrap_or_else(|| humanise(key)),
        Text::Mod(text) => mod_text(text).to_owned(),
    }
}

/// A mod string, in the only language the mod carries.
pub(crate) const fn mod_text(text: ModText) -> &'static str {
    text.english()
}

/// The English stand-in for a game key: its name without the family prefix, spaced, and
/// capitalised (`buttonhint_fire_grapple` reads "Fire grapple").
pub(crate) fn humanise(key: &str) -> String {
    let name = KEY_PREFIXES
        .iter()
        .find_map(|prefix| key.strip_prefix(prefix))
        .unwrap_or(key);
    let spaced = name.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The key families the summary uses, stripped by [`humanise`].
const KEY_PREFIXES: [&str; 2] = ["buttonhint_", "gui_settings_controller_mapping_"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_humanises_without_its_family() {
        assert_eq!(humanise("buttonhint_fire_grapple"), "Fire grapple");
        assert_eq!(
            humanise("gui_settings_controller_mapping_on_foot"),
            "On foot"
        );
        assert_eq!(humanise("plain"), "Plain");
        assert_eq!(humanise(""), "");
    }

    #[test]
    fn a_missing_key_falls_back_to_english() {
        let found = |_: &str| Some("Sauter".to_owned());
        let missing = |_: &str| None;
        let jump = Text::Game("buttonhint_jump");
        assert_eq!(resolve(jump, Some(Locale::LOCALE_FRENCH), found), "Sauter");
        assert_eq!(resolve(jump, Some(Locale::LOCALE_FRENCH), missing), "Jump");
        assert_eq!(resolve(jump, None, found), "Jump");
    }

    #[test]
    fn languages_the_fonts_cannot_draw_fall_back_to_english() {
        let found = |_: &str| Some("ジャンプ".to_owned());
        let jump = Text::Game("buttonhint_jump");
        assert_eq!(resolve(jump, Some(Locale::LOCALE_JAPANESE), found), "Jump");
        assert_eq!(resolve(jump, Some(Locale::LOCALE_ARABIC), found), "Jump");
        assert!(renders(Locale::LOCALE_RUSSIAN));
        assert_eq!(
            resolve(Text::Mod(ModText::Turn), Some(Locale::LOCALE_ARABIC), found),
            "Turn"
        );
    }
}
