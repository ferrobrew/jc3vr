//! Resolving the summary's strings: the game's through its localisation store
//! ([`Locator::GetLocalizedStringRaw`]), the mod's from [`ModText`].
//!
//! The panel draws with Libre Franklin and egui's bundled fallbacks, which cover the Latin and
//! Cyrillic languages but not Japanese or Arabic (egui neither carries a CJK font nor shapes Arabic).
//! In those two languages every game string falls back to its English stand-in, the key's own name
//! made readable ([`humanise`]), as does a key the game does not have.

use std::ffi::{CStr, c_char};

use jc3gi::{
    hash::hashlittle,
    ui::locator::{Locale, Locator},
};

use crate::controllers::summary::labels::{ModText, Text};

/// The game's current language, or `None` before the localisation store exists.
///
/// # Safety
///
/// Call from a game thread past startup.
pub unsafe fn current_locale() -> Option<Locale> {
    // SAFETY: the store is a singleton that lives for the game's lifetime once created.
    unsafe { Locator::get().map(|locator| locator.m_CurrentLocale) }
}

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

/// The game's string for `key` in its current language, or `None` when it has none.
///
/// # Safety
///
/// Call from a game thread past startup. The store takes its own reader lock.
pub unsafe fn game_string(key: &str) -> Option<String> {
    // The store keys strings by the low 32 bits of the key's hash.
    let hash = hashlittle(key.as_bytes()) as u32;
    // SAFETY: per the contract.
    unsafe { game_string_by_hash(hash) }
}

/// The game's string for the key whose hash is `hash`, in its current language, or `None` when it
/// has none: a key the game hands over already hashed, as a button hint's description.
///
/// # Safety
///
/// As [`game_string`].
pub unsafe fn game_string_by_hash(hash: u32) -> Option<String> {
    // SAFETY: per the contract; the returned string is owned by a loaded table and copied out at
    // once.
    unsafe {
        let locator = Locator::get()?;
        let raw = locator.GetLocalizedStringRaw(hash);
        if raw.is_null() {
            return None;
        }
        let text = CStr::from_ptr(raw as *const c_char).to_str().ok()?;
        (!text.is_empty()).then(|| text.to_owned())
    }
}

/// Whether the panel's fonts can draw `locale`'s strings.
pub const fn renders(locale: Locale) -> bool {
    !matches!(locale, Locale::LOCALE_JAPANESE | Locale::LOCALE_ARABIC)
}

/// A mod string, in the only language the mod carries.
pub const fn mod_text(text: ModText) -> &'static str {
    text.english()
}

/// The English stand-in for a game key: its name without the family prefix, spaced, and
/// capitalised (`buttonhint_fire_grapple` reads "Fire grapple").
pub fn humanise(key: &str) -> String {
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
