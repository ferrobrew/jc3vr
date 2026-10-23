//! The ammunition auto-pickup, for every weapon in the loadout table. The design is the two-handed
//! inventory's "No automatic moves" (`docs/mod/input/two-handed-inventory.md`), the engine side in
//! `docs/engine/gameplay/item-pickup.md`.
//!
//! A weapon lying in the world gives its stored rounds to the reserve when the player walks over it
//! only while the player's inventory holds a weapon of the same ammunition type
//! ([`Inventory::HasWeaponWithAmmoType`]), which reads the slots alone; its one caller, the
//! pickup's automatic credit, credits the rounds itself. So the detour also answers yes for the
//! local player's inventory when a weapon the table has on the body, in a hand or on a socket, is
//! of that type. The types are published once per input tick as a mask, which the detour reads
//! without a lock, since the credit runs wherever the pickup interaction activates.

use std::sync::atomic::{AtomicU32, Ordering};

use detours_macro::detour;
use jc3gi::{
    character::inventory::Inventory,
    equipment::weapon::{AmmunitionType, WeaponBase},
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    hands::ik,
    loadout::{Place, live},
    weapons::inventory::carry,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&HAS_WEAPON_WITH_AMMO_TYPE_BINDER)
}

/// Publish the ammunition types of the weapons the table has on the body. Called each input tick
/// while the table runs, after the reconcile, when every such weapon is kept alive by a slot or
/// the carry set.
///
/// # Safety
///
/// `slots` must be the local player's live slot weapons, on the main thread outside the world
/// update.
pub(crate) unsafe fn publish(slots: &[usize]) {
    let mut mask = 0;
    for (object, place) in live::bound() {
        let object = object.get();
        if matches!(place, Place::World(_)) || !(carry::contains(object) || slots.contains(&object))
        {
            continue;
        }
        // SAFETY: a slot or the carry set keeps the weapon alive.
        let weapon = unsafe { &*(object as *const WeaponBase) };
        // Read as the raw value: an unset type is out of the enum's range.
        let raw = unsafe { (&raw const weapon.m_AmmunitionType).cast::<i32>().read() };
        if let Some(bit) = bit(raw) {
            mask |= bit;
        }
    }
    MASK.store(mask, Ordering::Relaxed);
}

/// Forget the published types. Called when the table stops.
pub(crate) fn clear() {
    MASK.store(0, Ordering::Relaxed);
}

/// The ammunition types of the weapons the table has on the body, one bit per type.
static MASK: AtomicU32 = AtomicU32::new(0);

/// The mask's bit for the raw ammunition type `raw`, for the types a reserve holds.
fn bit(raw: i32) -> Option<u32> {
    (0..AmmunitionType::NOF_AMMO_TYPES as i32)
        .contains(&raw)
        .then(|| 1 << raw)
}

/// The ammunition type is taken as the raw value, so that one the enum does not name is not
/// misread.
#[detour(address = jc3gi::character::inventory::Inventory::HasWeaponWithAmmoType_ADDRESS)]
fn has_weapon_with_ammo_type(inventory: *const Inventory, ammo_type: i32) -> bool {
    if HAS_WEAPON_WITH_AMMO_TYPE
        .get()
        .unwrap()
        .call(inventory, ammo_type)
    {
        return true;
    }
    let local = ik::local_character();
    // SAFETY: the check is given a live inventory.
    let is_local = local != 0
        && unsafe { inventory.as_ref() }.is_some_and(|i| i.m_Character as usize == local);
    is_local && bit(ammo_type).is_some_and(|bit| MASK.load(Ordering::Relaxed) & bit != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_reserve_s_types_have_a_bit() {
        assert_eq!(bit(0), Some(1));
        assert_eq!(bit(AmmunitionType::NOF_AMMO_TYPES as i32), None);
        assert_eq!(bit(-1), None);
    }
}
