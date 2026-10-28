use std::{mem::MaybeUninit, ptr};

use jc3gi::equipment::weapon::WeaponClass;

use super::*;

fn weapon() -> Box<MaybeUninit<WeaponBase>> {
    let mut storage = Box::new(MaybeUninit::<WeaponBase>::zeroed());
    // SAFETY: the remaining mapped fields permit zero; initialize the nonzero-valid class first.
    unsafe {
        (&raw mut (*storage.as_mut_ptr()).m_Class).write(WeaponClass::E_WEAPON_CLASS_NONE);
    }
    storage
}

fn observed(object: ObjectKey) -> GrantTick {
    GrantTick {
        enabled: true,
        slot: Some(object),
        single: false,
        guns: true,
        stashed: false,
        socket_free: true,
        dt: 0.0,
    }
}

fn revalidate_and_write(inventory: &Inventory, object: ObjectKey) -> bool {
    let Some(mut handle) = matching_inventory_slot(inventory, object) else {
        return false;
    };
    // SAFETY: each test retains distinct weapon storage and its slot for this serialized phase.
    unsafe { handle.as_mut().m_MagazineAmmo = 7 };
    true
}

#[test]
fn one_grant_identity_cannot_mutate_a_missing_or_replaced_slot() {
    // SAFETY: Inventory's mapped enums contain zero and its other fields are zero-valid.
    let mut inventory: Inventory = unsafe { std::mem::zeroed() };
    let mut original = weapon();
    let mut replacement = weapon();
    let source = original.as_mut_ptr();
    let other = replacement.as_mut_ptr();
    let object = source as ObjectKey;
    inventory.m_Weapons[WeaponSlot::E_SLOT_DUAL_WIELD as usize].px = source;
    let mut grants = Grants::new();
    grants.cover_respawn(Some(true));
    let Grant::One {
        object: granted, ..
    } = grants.observe(observed(object))
    else {
        panic!("the respawn must produce a One grant");
    };
    for current in [ptr::null_mut(), other] {
        inventory.m_Weapons[WeaponSlot::E_SLOT_DUAL_WIELD as usize].px = current;
        assert!(!revalidate_and_write(&inventory, granted));
        // SAFETY: storage remains live, and the helper did not retain a borrow.
        assert_eq!(unsafe { (*source).m_MagazineAmmo }, 0);
        assert_eq!(unsafe { (*other).m_MagazineAmmo }, 0);
    }
    inventory.m_Weapons[WeaponSlot::E_SLOT_DUAL_WIELD as usize].px = source;
    assert_eq!(
        matching_inventory_slot(&inventory, granted)
            .unwrap()
            .as_ptr(),
        source
    );
    assert!(revalidate_and_write(&inventory, granted));
    assert_eq!(unsafe { (*source).m_MagazineAmmo }, 7);
}

#[test]
fn arrived_pair_identity_is_revalidated_after_native_pickup_phase() {
    // SAFETY: Inventory's mapped enums contain zero and its other fields are zero-valid.
    let mut inventory: Inventory = unsafe { std::mem::zeroed() };
    let mut original = weapon();
    let mut replacement = weapon();
    let source = original.as_mut_ptr();
    let other = replacement.as_mut_ptr();
    let object = source as ObjectKey;
    inventory.m_Weapons[WeaponSlot::E_SLOT_DUAL_WIELD as usize].px = source;
    let mut grants = Grants::new();
    assert_eq!(grants.observe(observed(object)), Grant::Request { object });
    let pair = grants.arrived(Some(object), false).unwrap();
    // The shared observation ends before native pickup can replace the slot.
    assert_eq!(
        unsafe {
            matching_inventory_slot(&inventory, pair)
                .unwrap()
                .as_ref()
                .m_MagazineAmmo
        },
        0
    );
    for current in [ptr::null_mut(), other] {
        inventory.m_Weapons[WeaponSlot::E_SLOT_DUAL_WIELD as usize].px = current;
        assert!(!revalidate_and_write(&inventory, pair));
        assert_eq!(unsafe { (*source).m_MagazineAmmo }, 0);
        assert_eq!(unsafe { (*other).m_MagazineAmmo }, 0);
    }
    inventory.m_Weapons[WeaponSlot::E_SLOT_DUAL_WIELD as usize].px = source;
    assert!(revalidate_and_write(&inventory, pair));
    assert_eq!(unsafe { (*source).m_MagazineAmmo }, 7);
}
