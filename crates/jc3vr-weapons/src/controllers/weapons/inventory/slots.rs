//! The inventory's weapon slots ([`Inventory::m_Weapons`]), read in one place: a slot by its
//! [`WeaponSlot`], and the slots with their weapons.
//!
//! A slot value past the eight slots ([`WeaponSlot::E_NUM_WEAPON_SLOTS`], [`WeaponSlot::E_NONE`])
//! reads as an empty slot.

use std::ptr::NonNull;

use jc3gi::{
    character::inventory::{Inventory, WeaponSlot},
    equipment::weapon::WeaponBase,
    types::shared_ptr::SharedPtr,
};

use crate::controllers::{loadout::SLOTS, weapons::inventory::engine::ObjectKey};

/// The weapon in `inventory`'s `slot`.
pub(crate) fn weapon(inventory: &Inventory, slot: WeaponSlot) -> Option<NonNull<WeaponBase>> {
    NonNull::new(inventory.m_Weapons.get(slot as usize)?.px)
}

/// The address of the weapon in `inventory`'s `slot`, the key the inventory's stores keep it by.
pub(crate) fn object(inventory: &Inventory, slot: WeaponSlot) -> Option<ObjectKey> {
    weapon(inventory, slot).map(|weapon| weapon.as_ptr() as ObjectKey)
}

/// Whether `inventory`'s `slot` holds the weapon at `weapon`.
pub(crate) fn holds(inventory: &Inventory, slot: WeaponSlot, weapon: ObjectKey) -> bool {
    object(inventory, slot) == Some(weapon)
}

/// Each of `inventory`'s slots with its strong reference, which is null for an empty slot.
pub(crate) fn entries(
    inventory: &Inventory,
) -> impl Iterator<Item = (WeaponSlot, &SharedPtr<WeaponBase>)> {
    SLOTS.into_iter().zip(inventory.m_Weapons.iter())
}

/// Each of `inventory`'s slots that holds a weapon, with it.
pub(crate) fn occupied(
    inventory: &Inventory,
) -> impl Iterator<Item = (WeaponSlot, NonNull<WeaponBase>)> {
    entries(inventory).filter_map(|(slot, entry)| Some((slot, NonNull::new(entry.px)?)))
}

/// The slot of `inventory`'s that holds the weapon at `object`, with it.
pub(crate) fn find(
    inventory: &Inventory,
    object: ObjectKey,
) -> Option<(WeaponSlot, NonNull<WeaponBase>)> {
    occupied(inventory).find(|(_, weapon)| weapon.as_ptr() as ObjectKey == object)
}

/// The original native pointer of the slot of `inventory`'s that holds the weapon at `object`,
/// without a pointee borrow. The caller must retain the slot source and serialize native access
/// while using the handle.
pub(crate) fn handle(inventory: &Inventory, object: ObjectKey) -> Option<NonNull<WeaponBase>> {
    find(inventory, object).map(|(_, weapon)| weapon)
}

/// Whether one of `inventory`'s slots holds the weapon at `object`.
pub(crate) fn contains(inventory: &Inventory, object: ObjectKey) -> bool {
    find(inventory, object).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An inventory with a weapon at `object` in each of `filled`, never dereferenced.
    fn inventory(filled: &[(WeaponSlot, ObjectKey)]) -> Inventory {
        // SAFETY: the inventory's mapped enums contain zero and its other fields are zero-valid.
        let mut inventory: Inventory = unsafe { std::mem::zeroed() };
        for &(slot, object) in filled {
            inventory.m_Weapons[slot as usize].px = object as *mut WeaponBase;
        }
        inventory
    }

    #[test]
    fn a_slot_reads_its_own_entry_and_a_value_past_the_slots_reads_empty() {
        let inventory = inventory(&[
            (WeaponSlot::E_SLOT_TWO_HANDED, 0x10),
            (WeaponSlot::E_SLOT_BEACON, 0x20),
        ]);
        assert_eq!(
            object(&inventory, WeaponSlot::E_SLOT_TWO_HANDED),
            Some(0x10)
        );
        assert_eq!(object(&inventory, WeaponSlot::E_SLOT_BEACON), Some(0x20));
        assert_eq!(object(&inventory, WeaponSlot::E_SLOT_DUAL_WIELD), None);
        assert_eq!(object(&inventory, WeaponSlot::E_NUM_WEAPON_SLOTS), None);
        assert_eq!(object(&inventory, WeaponSlot::E_NONE), None);
        assert!(holds(&inventory, WeaponSlot::E_SLOT_BEACON, 0x20));
        assert!(!holds(&inventory, WeaponSlot::E_SLOT_TWO_HANDED, 0x20));
    }

    #[test]
    fn the_occupied_slots_are_named_by_their_own_slot() {
        let inventory = inventory(&[
            (WeaponSlot::E_SLOT_HEAVY, 0x30),
            (WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE, 0x40),
        ]);
        let occupied: Vec<_> = occupied(&inventory)
            .map(|(slot, weapon)| (slot, weapon.as_ptr() as ObjectKey))
            .collect();
        assert_eq!(
            occupied,
            [
                (WeaponSlot::E_SLOT_HEAVY, 0x30),
                (WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE, 0x40)
            ]
        );
        assert_eq!(
            find(&inventory, 0x40).map(|(slot, _)| slot),
            Some(WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE)
        );
        assert!(contains(&inventory, 0x30));
        assert!(!contains(&inventory, 0x50));
    }
}
