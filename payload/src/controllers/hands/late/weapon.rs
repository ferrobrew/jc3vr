//! Re-pushing the held gun's model matrix with the same correction the palette edit applies to the
//! hand bones, so the gun and the hand never separate.

use std::ffi::c_void;

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    character::character::SafeBoneIndex,
    equipment::weapon::{WeaponBase, WeaponState},
    game::GameObjectRenderContext,
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    frame::Hand,
    hands::{
        ik,
        late::{
            frame,
            math::{lerp_transform, pose_matrix},
        },
    },
    weapons::hold::second,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&WEAPON_UPDATE_RENDER_BINDER)
}

/// The hand an attach bone (an `NBone::ESafeBoneIndex`) belongs to, or `None` for a holster or
/// other non-hand bone.
pub(crate) fn attach_hand(bone: u32) -> Option<Hand> {
    [
        (SafeBoneIndex::ATTACH_HAND_LEFT as u32, Hand::Left),
        (SafeBoneIndex::ATTACH_HAND_LEFT2 as u32, Hand::Left),
        (SafeBoneIndex::ATTACH_HAND_RIGHT as u32, Hand::Right),
        (SafeBoneIndex::ATTACH_HAND_RIGHT2 as u32, Hand::Right),
    ]
    .into_iter()
    .find(|&(safe, _)| safe == bone)
    .map(|(_, hand)| hand)
}

/// The engine's weapon render update, then the gun's model matrix re-pushed with the hand's
/// correction.
#[detour(address = jc3gi::equipment::weapon::WeaponBase::UpdateRender_ADDRESS)]
fn weapon_update_render(game_object: *mut c_void, context: *const GameObjectRenderContext) {
    WEAPON_UPDATE_RENDER
        .get()
        .unwrap()
        .call(game_object, context);
    unsafe { late_update_weapon(game_object, context) };
}

/// Re-push the model matrix of each gun of `game_object`'s weapon that the local player wields in
/// a hand attach bone, with that hand's correction.
unsafe fn late_update_weapon(game_object: *mut c_void, context: *const GameObjectRenderContext) {
    let (Some(context), false) = (unsafe { context.as_ref() }, game_object.is_null()) else {
        return;
    };
    let weapon = unsafe {
        &*(game_object
            .cast::<u8>()
            .sub(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
            .cast::<WeaponBase>())
    };
    let local = ik::local_character();
    // `m_User` is a `weak_ptr<CCharacter>`; its first word is the character. Checked before the
    // frame lock, since this runs for every weapon in the world.
    if local == 0 || weapon.m_User[0] as usize != local {
        return;
    }
    let Some(frame) = frame::current() else {
        return;
    };
    let state = weapon.m_State;
    if !matches!(
        state,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    ) && !second::is(weapon as *const WeaponBase as usize)
    {
        return;
    }
    let dtf = context.m_Dtf;
    let guns = &weapon.m_lWeaponData;
    let mut cursor = guns.m_Begin;
    while !cursor.is_null() && cursor < guns.m_End {
        let gun = unsafe { (*cursor).as_ref() };
        cursor = unsafe { cursor.add(1) };
        let Some(gun) = gun else {
            continue;
        };
        let Some(&bone) = gun.m_lBoneAttachementPerWeaponState.get(state as usize) else {
            continue;
        };
        let Some(hand) = attach_hand(bone) else {
            continue;
        };
        let (Some(d), Some(instance)) = (frame.hand_delta(hand, dtf), unsafe {
            gun.m_BaseModelInstance.as_mut()
        }) else {
            continue;
        };
        let interpolated = lerp_transform(
            Mat4::from(gun.m_WorldMatrixT0),
            Mat4::from(gun.m_WorldMatrixT1),
            dtf,
        );
        let matrix = Matrix4::from(pose_matrix(d) * interpolated);
        unsafe { instance.UpdateMatrix(&matrix) };
        frame::count_weapon_update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_bones_map_to_hands() {
        assert_eq!(
            attach_hand(SafeBoneIndex::ATTACH_HAND_RIGHT2 as u32),
            Some(Hand::Right)
        );
        assert_eq!(
            attach_hand(SafeBoneIndex::ATTACH_HAND_LEFT as u32),
            Some(Hand::Left)
        );
        assert_eq!(attach_hand(SafeBoneIndex::HEAD as u32), None);
    }
}
