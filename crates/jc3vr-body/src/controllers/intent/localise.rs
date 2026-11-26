//! Reading the game's localised strings: a key through its localisation store
//! ([`Locator::GetLocalizedStringRaw`]), and a weapon's name through the menu's lookup the HUD uses
//! ([`UIMenu::GetWeaponNameById`]).
//!
//! The mod's panels draw with Libre Franklin and egui's bundled fallbacks, which cover the Latin
//! and Cyrillic languages but not Japanese or Arabic (egui neither carries a CJK font nor shapes
//! Arabic); [`renders`] says which languages they can draw, and a weapon's name reads as nothing in
//! the others.

use std::ffi::{CStr, c_char};

use jc3gi::ui::{
    locator::{Locale, Locator},
    ui_menu::UIMenu,
};
use jc3vr_core::hash::hashlittle;

/// The game's current language, or `None` before the localisation store exists.
///
/// # Safety
///
/// Call from a game thread past startup.
pub unsafe fn current_locale() -> Option<Locale> {
    // SAFETY: the store is a singleton that lives for the game's lifetime once created.
    unsafe { Locator::get().map(|locator| locator.m_CurrentLocale) }
}

/// The game's string for `key` in its current language, or `None` when it has none.
///
/// # Safety
///
/// Call from a game thread past startup. The store takes its own reader lock.
pub unsafe fn game_string(key: &str) -> Option<String> {
    let hash = hashlittle(key.as_bytes());
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

/// The display name of the weapon type whose name hash is `name_hash`
/// ([`WeaponBase::m_WeaponNameHash`](jc3gi::equipment::weapon::WeaponBase::m_WeaponNameHash)), as
/// the HUD shows it, or `None` when the store has none or the panel's fonts cannot draw the
/// current language.
///
/// # Safety
///
/// As [`game_string`].
pub unsafe fn weapon_name(name_hash: u32) -> Option<String> {
    // SAFETY: per the contract.
    let locale = unsafe { current_locale() }?;
    if !renders(locale) {
        return None;
    }
    // SAFETY: per the contract; the string is the store's, or the lookup's own empty fallback, and
    // is copied out at once.
    unsafe {
        let raw = UIMenu::GetWeaponNameById(name_hash);
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
