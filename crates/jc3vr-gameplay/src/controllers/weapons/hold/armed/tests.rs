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
fn missing_targets_stop_unique_guns_once_across_both_phases_and_repeated_frames() {
    with_weapon(|weapon, first, second| {
        let first = first as *mut WeaponData;
        let second = second as *mut WeaponData;
        for pair in [[first, first], [first, second], [ptr::null_mut(), second]] {
            let mut pair = pair;
            entries(weapon, &mut pair);
            let expected: Vec<_> = unsafe { gun_handles(weapon) }
                .into_iter()
                .flatten()
                .map(|gun| gun.as_ptr())
                .collect();
            weapon.m_IsFiring = true;
            let mut stopped = Vec::new();
            for _frame in 0..3 {
                for _phase in 0..2 {
                    unsafe {
                        release_fire_with(weapon, |weapon, gun| {
                            stopped.push(gun);
                            weapon.m_IsFiring = false;
                        })
                    };
                }
            }
            assert_eq!(stopped, expected);
            // A new native firing transition resumes cancellation without a mod-side identity cache.
            weapon.m_IsFiring = true;
            unsafe {
                release_fire_with(weapon, |weapon, gun| {
                    stopped.push(gun);
                    weapon.m_IsFiring = false;
                })
            };
            assert_eq!(stopped.len(), expected.len() * 2);
        }
    });
}

#[test]
fn unavailable_target_releases_fired_semi_auto_and_auto_input_and_pending_shots() {
    with_weapon(|weapon, first, _| {
        let mut pair = [first as *mut WeaponData; 2];
        entries(weapon, &mut pair);
        for automatic in [false, true] {
            weapon.m_IsFiring = true;
            weapon.m_IsTryingToFire = true;
            weapon.m_IsTriggerPulled = true;
            weapon.m_FiredThisInput = true;
            weapon.m_IsFiringCancelled = true;
            weapon.m_IsFeatheringTrigger = true;
            weapon.m_InputBufferTimer = 0.4;
            weapon.m_ForceFireProjectile = true;
            weapon.m_ForceFireTimer = 0.7;
            weapon.m_ForceFireCount = 3;
            first.m_BurstShotsLeft = 4;
            let mut stops = 0;
            unsafe {
                release_fire_with(weapon, |weapon, _| {
                    stops += 1;
                    weapon.m_IsFiring = false;
                })
            };
            assert_eq!(stops, 1);
            assert!(!weapon.m_IsTriggerPulled);
            assert!(!weapon.m_FiredThisInput);
            assert!(!weapon.m_IsFiringCancelled);
            assert!(!weapon.m_IsTryingToFire);
            assert!(!weapon.m_IsFeatheringTrigger);
            assert!(!weapon.m_ForceFireProjectile);
            assert_eq!(weapon.m_InputBufferTimer, 0.0);
            assert_eq!(weapon.m_ForceFireTimer, 0.0);
            assert_eq!(weapon.m_ForceFireCount, 0);
            assert_eq!(first.m_BurstShotsLeft, 0);
            // These are the mapped gates read by UpdateWeapon_Parallel and Fire_StartBase on recovery.
            let pulled = 1.0 > 0.5;
            let requests_shot =
                pulled && (automatic || !weapon.m_FiredThisInput) && !weapon.m_IsFiringCancelled;
            assert!(requests_shot);
            let first_shot = !weapon.m_IsFiring;
            assert!(first_shot);
        }
    });
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
