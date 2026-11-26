//! The local player's own bullets can hit the local player.
//!
//! A bullet ignores the bodies its weapon's holder names ([`WeaponBase::GetHolderInformation`]),
//! so a character's bullets pass through that character. With the gun in a tracked hand, the
//! player can point it at their own body, and the shot should land: under the controller scheme,
//! the bullet weapons' shot ([`BulletWeaponBase::Fire_Projectile`]) of a weapon whose user is the
//! local player drops the local player's character from the ignored bodies, and the hit then takes
//! the game's own damage path. The struck character skips its same-faction filter for damage the
//! local player deals, so the hit lands at full strength.
//!
//! Only that one call is changed, and only a character holder's entry: the other callers of the
//! holder query (the magnet's gravity field, the laser's beam length, the anti-air gun) keep the
//! holder ignored, and a vehicle holder's list (the vehicle, then its driver) is left whole, so a
//! vehicle's guns still pass through the vehicle and its driver. Gamepad play keeps the game's
//! behaviour.

use std::{cell::Cell, ffi::c_void};

use detours_macro::detour;
use jc3gi::{
    character::character::Character,
    equipment::weapon::{BulletWeaponBase, WeaponBase, WeaponData},
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::scheme;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&BULLET_FIRE_PROJECTILE_BINDER)
        .with_static_binder(&GET_HOLDER_INFORMATION_BINDER)
}

thread_local! {
    /// Set while a bullet weapon's shot runs on this thread.
    static IN_BULLET_SHOT: Cell<bool> = const { Cell::new(false) };
}

#[detour(address = jc3gi::equipment::weapon::BulletWeaponBase::Fire_Projectile_ADDRESS)]
fn bullet_fire_projectile(weapon: *mut BulletWeaponBase, gun_data: *mut WeaponData) {
    let outer = IN_BULLET_SHOT.replace(true);
    BULLET_FIRE_PROJECTILE.get().unwrap().call(weapon, gun_data);
    IN_BULLET_SHOT.set(outer);
}

#[detour(address = jc3gi::equipment::weapon::WeaponBase::GetHolderInformation_ADDRESS)]
fn get_holder_information(
    weapon: *mut WeaponBase,
    faction: *mut i32,
    peer_id: *mut u8,
    is_player: *mut bool,
    ignore: *mut [*const c_void; 2],
) {
    GET_HOLDER_INFORMATION
        .get()
        .unwrap()
        .call(weapon, faction, peer_id, is_player, ignore);
    if !IN_BULLET_SHOT.get() {
        return;
    }
    // SAFETY: the weapon is live for the length of its own shot, on this thread.
    let Some(weapon) = (unsafe { weapon.as_ref() }) else {
        return;
    };
    if !weapon.m_IsUserLocalPlayer || !scheme::is_controllers() {
        return;
    }
    // SAFETY: the game's own query of the local player, on the thread that updates the weapon.
    let player = unsafe { Character::GetLocalPlayerCharacter() };
    // SAFETY: the caller's two-entry list, which the original call just filled.
    if let Some(ignore) = unsafe { ignore.as_mut() } {
        drop_character_holder(ignore, player as *const c_void);
    }
}

/// Clear `ignore` when it is a character holder's list naming `character`: the character first
/// and nothing after it. A vehicle holder's list names the vehicle first and is left alone.
fn drop_character_holder(ignore: &mut [*const c_void; 2], character: *const c_void) {
    if !character.is_null() && ignore[0] == character && ignore[1].is_null() {
        ignore[0] = std::ptr::null();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: *const c_void = 0x1000 as *const c_void;
    const VEHICLE: *const c_void = 0x2000 as *const c_void;

    #[test]
    fn the_player_holding_the_gun_is_no_longer_ignored() {
        let mut ignore = [PLAYER, std::ptr::null()];
        drop_character_holder(&mut ignore, PLAYER);
        assert_eq!(ignore, [std::ptr::null(); 2]);
    }

    #[test]
    fn a_vehicle_holder_keeps_its_vehicle_and_driver() {
        let mut ignore = [VEHICLE, PLAYER];
        drop_character_holder(&mut ignore, PLAYER);
        assert_eq!(ignore, [VEHICLE, PLAYER]);
    }

    #[test]
    fn another_character_or_no_player_changes_nothing() {
        let mut ignore = [VEHICLE, std::ptr::null()];
        drop_character_holder(&mut ignore, PLAYER);
        assert_eq!(ignore, [VEHICLE, std::ptr::null()]);
        let mut ignore = [std::ptr::null(); 2];
        drop_character_holder(&mut ignore, std::ptr::null());
        assert_eq!(ignore, [std::ptr::null(); 2]);
    }
}
