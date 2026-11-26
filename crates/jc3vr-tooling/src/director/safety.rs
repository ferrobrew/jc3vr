//! Actor safety: god mode and unlimited ammo on the player's character, re-asserted every frame.
//!
//! Both are the game's own facilities (`CCharacter::SetGodMode`, `CCharacter::SetUnlimitedAmmo`),
//! called exactly as the player object calls them from its debug-menu path. Re-asserting rather
//! than setting once matters because a respawn recreates the character with the flags clear, and
//! sequences toggle god mode themselves. When the config turns a flag off the pass clears it once
//! and then leaves the game's own state alone, so a sequence-driven god mode is not fought.

use jc3gi::character::character::{Character, DamageStateFlags};
use parking_lot::Mutex;

use crate::director::DirectorConfig;

/// What the last pass asserted, so a config edge from on to off clears the flag exactly once.
static ASSERTED: Mutex<Asserted> = Mutex::new(Asserted {
    god_mode: false,
    unlimited_ammo: false,
});

struct Asserted {
    god_mode: bool,
    unlimited_ammo: bool,
}

/// Reconcile the player's character against the config. Main thread.
pub(super) fn apply(cfg: &DirectorConfig) {
    // SAFETY: the local player character is game-thread data, read and written on the main thread.
    unsafe {
        let Some(character) = Character::GetLocalPlayerCharacter().as_mut() else {
            return;
        };
        let mut asserted = ASSERTED.lock();

        let god_mode_on = character.m_DamageFlags.contains(DamageStateFlags::GOD_MODE);
        if cfg.god_mode && !god_mode_on {
            character.SetGodMode(true);
            tracing::info!("director: god mode ON");
        } else if !cfg.god_mode && asserted.god_mode && god_mode_on {
            character.SetGodMode(false);
            tracing::info!("director: god mode OFF");
        }
        asserted.god_mode = cfg.god_mode;

        if cfg.unlimited_ammo && !character.m_UnlimitedAmmo {
            character.SetUnlimitedAmmo(true);
            tracing::info!("director: unlimited ammo ON");
        } else if !cfg.unlimited_ammo && asserted.unlimited_ammo && character.m_UnlimitedAmmo {
            character.SetUnlimitedAmmo(false);
            tracing::info!("director: unlimited ammo OFF");
        }
        asserted.unlimited_ammo = cfg.unlimited_ammo;
    }
}

/// The live state of both flags on the player's character, for the debug tab.
pub fn live_state() -> Option<(bool, bool)> {
    // SAFETY: read-only access to game-thread data from the UI pass on the same thread.
    unsafe {
        let character = Character::GetLocalPlayerCharacter().as_ref()?;
        Some((
            character.m_DamageFlags.contains(DamageStateFlags::GOD_MODE),
            character.m_UnlimitedAmmo,
        ))
    }
}
