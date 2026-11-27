//! The native weapon object's accessors: the weapon behind a `CGameObject` subobject and the
//! subobject of a weapon, and the guns in its gun list. Pure pointer reads, shared by the hands'
//! render-time late update and the weapon handling.

use std::ptr::NonNull;

use jc3gi::{
    equipment::weapon::{WeaponBase, WeaponData},
    game::GameObject,
};

/// The weapon whose `CGameObject` subobject is `object`, or null for a null object. The caller
/// establishes that the object is a weapon.
pub fn weapon_at(object: *const GameObject) -> *mut WeaponBase {
    if object.is_null() {
        return std::ptr::null_mut();
    }
    object
        .cast::<u8>()
        .wrapping_sub(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
        .cast::<WeaponBase>()
        .cast_mut()
}

/// `weapon`'s `CGameObject` subobject, the object that references, holders, and spawn calls name
/// it by.
#[inline]
pub fn object_of(weapon: *const WeaponBase) -> *mut GameObject {
    weapon
        .cast::<u8>()
        .wrapping_add(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
        .cast::<GameObject>()
        .cast_mut()
}

/// How many guns `weapon`'s gun list holds, a single pistol's hidden second gun included.
///
/// # Safety
///
/// `weapon` must be live.
pub unsafe fn gun_count(weapon: &WeaponBase) -> usize {
    let list = &weapon.m_lWeaponData;
    if list.m_Begin.is_null() || list.m_End < list.m_Begin {
        0
    } else {
        unsafe { list.m_End.offset_from(list.m_Begin) as usize }
    }
}

/// Gun `index` of `weapon`, as its list points to it: each gun is its own object, not part of the
/// weapon.
///
/// # Safety
///
/// `weapon` must be live.
pub unsafe fn gun_ptr(weapon: &WeaponBase, index: usize) -> Option<NonNull<WeaponData>> {
    if index >= unsafe { gun_count(weapon) } {
        return None;
    }
    NonNull::new(unsafe { *weapon.m_lWeaponData.m_Begin.add(index) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_object_casts_round_trip_and_keep_null() {
        let weapon = 0x1000 as *const WeaponBase;
        assert_eq!(weapon_at(object_of(weapon)).cast_const(), weapon);
        assert!(weapon_at(std::ptr::null()).is_null());
    }
}
