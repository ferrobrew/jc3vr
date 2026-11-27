//! Under the controller scheme, weapons are picked up only by hand: the game's weapon pickup panel
//! (the offered weapon, the one it replaces, and the use-item glyph) is hidden, and the use-item
//! button no longer picks a weapon up. The design is in `docs/mod/input/physical-hands.md`.
//!
//! The panel is cleared in place of the player's pickup readout ([`Player::UpdateWeaponHUD`]),
//! which otherwise shows it whenever the character's interaction entries offer a weapon. Clearing
//! its remembered offer as well means the game shows the panel again on its next update once the
//! gamepad scheme returns.
//!
//! The button's pickup is the weapon handling's: it queues the pickup act on a positive use-item
//! weapon input ([`Inventory::UpdateWeaponHandling`]). For the length of that call the input reads
//! zero, and it is put back afterwards for anything that reads it later in the tick; the use
//! action itself is left alone, since it also enters vehicles and the other use interactions. The
//! one use-item click the mod sends on purpose, the physical hands' fallback for a weapon the direct
//! pickup refuses ([`grant`]), passes for a short window.
//!
//! [`Player::UpdateWeaponHUD`]: jc3gi::character::player::Player::UpdateWeaponHUD
//! [`Inventory::UpdateWeaponHandling`]: jc3gi::character::inventory::Inventory::UpdateWeaponHandling

use std::time::{Duration, Instant};

use detours_macro::detour;
use jc3gi::{
    character::{
        character::{Character, WeaponInput},
        player::Player,
    },
    ui::hud_ui::HUDUI,
};
use jc3vr_body::controllers::scheme;
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::weapons::hold::tuning::HOLD;

jc3vr_core::counters! {
    /// The button pickup suppression's readout.
    pub(crate) struct ButtonPickupCounters => pub struct ButtonPickupStatus {
        /// Weapon-handling calls whose use-item input was held back, since the payload loaded.
        held_back,
        /// Weapon-handling calls whose use-item input passed as the mod's own fallback click.
        granted,
        /// Frames the pickup panel was kept hidden.
        panel_hidden,
    }
}

pub fn status() -> ButtonPickupStatus {
    COUNTS.snapshot()
}

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_WEAPON_HUD_BINDER)
}

/// Whether the suppression acts now: the controller scheme is in effect.
pub fn suppressing() -> bool {
    scheme::enabled() && scheme::is_controllers()
}

/// Let the use-item clicks of the next [`grant_window`] through: the mod is clicking the action to
/// have the game pick up a weapon the hand reached.
pub(crate) fn grant() {
    *GRANT.lock() = Some(Instant::now());
}

/// The use-item input held back for one weapon-handling call, to be put back after it.
#[must_use]
pub struct HeldUseItem(f32);

impl HeldUseItem {
    /// Put the held input back.
    ///
    /// # Safety
    ///
    /// `character` must be the character the input was held back from, on the thread updating it.
    pub unsafe fn restore(self, character: &mut Character) {
        character.m_WeaponInputs[WeaponInput::E_INPUT_USE_ITEM as usize] = self.0;
    }
}

/// Hold back the local player's use-item input for the weapon-handling call about to run, unless
/// the mod's own fallback click is granted.
///
/// # Safety
///
/// `character` must be the live local player character, on the thread running its weapon
/// handling, just before the call.
pub unsafe fn hold_back(character: &mut Character) -> Option<HeldUseItem> {
    let input = &mut character.m_WeaponInputs[WeaponInput::E_INPUT_USE_ITEM as usize];
    if *input <= 0.0 || !suppressing() {
        return None;
    }
    if granted(*GRANT.lock(), Instant::now()) {
        COUNTS.granted.bump();
        return None;
    }
    let held = HeldUseItem(*input);
    *input = 0.0;
    COUNTS.held_back.bump();
    Some(held)
}

static GRANT: Mutex<Option<Instant>> = Mutex::new(None);
static COUNTS: ButtonPickupCounters = ButtonPickupCounters::new();

/// Whether a grant made at `at` still covers `now`.
fn granted(at: Option<Instant>, now: Instant) -> bool {
    at.is_some_and(|at| now.saturating_duration_since(at) <= grant_window())
}

/// How long a [`grant`] lets the use-item input through ([`use_item_grant_ms`](crate::controllers::weapons::hold::tuning::HoldTuning::use_item_grant_ms)).
fn grant_window() -> Duration {
    HOLD.get().hold.use_item_grant_ms.duration()
}

/// The player's pickup readout. While the suppression acts, the panel is cleared when it shows and
/// the remembered offer is reset instead of running the readout.
#[detour(address = jc3gi::character::player::Player::UpdateWeaponHUD_ADDRESS)]
fn update_weapon_hud(player: *mut Player) {
    // SAFETY: the game passes its live player.
    let Some(p) = (unsafe { player.as_mut() }).filter(|_| suppressing()) else {
        UPDATE_WEAPON_HUD.get().unwrap().call(player);
        return;
    };
    if p.m_CurrentWeaponPickupId != -1 {
        // SAFETY: the HUD is a singleton that lives for the game's lifetime once created; the
        // readout clears it from this same call.
        if let Some(hud) = unsafe { HUDUI::get() } {
            unsafe { hud.ClearValidWeaponPickup() };
        }
    }
    p.m_CurrentWeaponPickupId = -1;
    p.m_CurrentWeaponPickupAmmo = -1;
    COUNTS.panel_hidden.bump();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_covers_only_its_window() {
        let at = Instant::now();
        assert!(!granted(None, at));
        assert!(granted(Some(at), at));
        assert!(granted(Some(at), at + grant_window()));
        assert!(!granted(
            Some(at),
            at + grant_window() + Duration::from_millis(1)
        ));
    }

    #[test]
    fn a_grant_from_the_future_still_counts() {
        let now = Instant::now();
        assert!(granted(Some(now + Duration::from_millis(10)), now));
    }
}
