use std::{mem::MaybeUninit, ptr};

use jc3gi::equipment::weapon::WeaponClass;

use super::*;

fn with_weapon(visit: impl FnOnce(&mut WeaponBase, &mut WeaponData, &mut WeaponData)) {
    let mut storage = Box::new(MaybeUninit::<WeaponBase>::zeroed());
    // SAFETY: every mapped value permits zero except WeaponClass, initialized before the borrow.
    let weapon = unsafe {
        let weapon = storage.as_mut_ptr();
        (&raw mut (*weapon).m_Class).write(WeaponClass::E_WEAPON_CLASS_NONE);
        &mut *weapon
    };
    // SAFETY: WeaponData contains pointers, numbers, and zero-valid flags only.
    let mut first: Box<WeaponData> = Box::new(unsafe { std::mem::zeroed() });
    let mut second: Box<WeaponData> = Box::new(unsafe { std::mem::zeroed() });
    visit(weapon, first.as_mut(), second.as_mut());
}

fn entries(weapon: &mut WeaponBase, entries: &mut [*mut WeaponData]) {
    weapon.m_lWeaponData.m_Begin = entries.as_mut_ptr();
    weapon.m_lWeaponData.m_End = unsafe { entries.as_mut_ptr().add(entries.len()) };
}

#[test]
fn arm_disarm_layout_restores_original_bones_and_mirrored_scales_with_duplicates_and_holes() {
    with_weapon(|weapon, first, second| {
        let holstered = WeaponState::E_WS_HOLSTERED as usize;
        first.m_lBoneAttachementPerWeaponState[holstered] = 17;
        first.m_HolsteredScaleValue = -0.35;
        second.m_lBoneAttachementPerWeaponState[holstered] = 29;
        second.m_HolsteredScaleValue = 0.65;
        let first_ptr = first as *mut WeaponData;
        let second_ptr = second as *mut WeaponData;
        for mut pair in [
            [first_ptr, first_ptr],
            [ptr::null_mut(), second_ptr],
            [first_ptr, second_ptr],
        ] {
            entries(weapon, &mut pair);
            let saved = unsafe { arm_layout(weapon, Hand::Left) };
            for gun in unsafe { gun_handles(weapon) }.into_iter().flatten() {
                let gun = unsafe { gun.as_ref() };
                assert_eq!(gun.m_HolsteredScaleValue, 1.0);
                assert_eq!(
                    gun.m_lBoneAttachementPerWeaponState[holstered],
                    layout::hand_bone(Hand::Left)
                );
            }
            pair.reverse();
            unsafe { restore_layout(weapon, saved) };
            assert_eq!(first.m_lBoneAttachementPerWeaponState[holstered], 17);
            assert_eq!(first.m_HolsteredScaleValue, -0.35);
            assert_eq!(second.m_lBoneAttachementPerWeaponState[holstered], 29);
            assert_eq!(second.m_HolsteredScaleValue, 0.65);
        }
    });
}
