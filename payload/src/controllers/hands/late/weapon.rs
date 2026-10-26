//! Re-pushing the held gun's model matrix with the same correction the palette edit applies to the
//! hand bones, so the gun and the hand never separate.

use std::{ffi::c_void, time::Instant};

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    character::character::SafeBoneIndex,
    equipment::weapon::{WeaponBase, WeaponState},
    game::GameObjectRenderContext,
    types::math::Matrix4,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::{
        ik,
        late::{
            frame,
            math::{lerp_transform, pose_matrix},
        },
    },
    weapons::hold::{self, armed},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&WEAPON_UPDATE_RENDER_BINDER)
}

/// One gun mesh's submitted pose, for comparison with its identified current collision body.
#[derive(Debug, Copy, Clone)]
pub(crate) struct RenderedGun {
    pub weapon: usize,
    pub matrix: Mat4,
    pub fraction: f32,
    pub at: Instant,
}

pub(crate) fn rendered_guns() -> [Option<RenderedGun>; 2] {
    *RENDERED_GUNS.lock()
}

pub(super) fn begin_frame() {
    *RENDERED_GUNS.lock() = [None; 2];
}

/// The hand that owns this gun, retaining paired-gun placement through native state changes.
pub(crate) fn gun_hand(
    weapon: usize,
    state: WeaponState,
    bone: Option<u32>,
    index: usize,
    count: usize,
) -> Option<Hand> {
    let owner = hold::owned_holder(weapon).or_else(|| armed::hand_of(weapon));
    attachment_hand(state, bone, owner, index, count)
}

fn attachment_hand(
    state: WeaponState,
    bone: Option<u32>,
    owner: Option<Hand>,
    index: usize,
    count: usize,
) -> Option<Hand> {
    if !matches!(
        state,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    ) && owner.is_none()
    {
        return None;
    }
    owner
        .map(|hand| {
            if count > 1 && index % 2 == 1 {
                hand.other()
            } else {
                hand
            }
        })
        .or_else(|| bone.and_then(attach_hand))
}

/// The hand an attach bone belongs to, or `None` for a non-hand bone.
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
    let address = weapon as *const WeaponBase as usize;
    let owned = hold::owned_holder(address)
        .or_else(|| armed::hand_of(address))
        .is_some();
    if local == 0 || (weapon.m_User[0] as usize != local && !owned) {
        return;
    }
    let Some(frame) = frame::current() else {
        return;
    };
    let state = weapon.m_State;
    let dtf = context.m_Dtf;
    let guns = &weapon.m_lWeaponData;
    let mut cursor = guns.m_Begin;
    let count = unsafe { gun::guns(weapon) }.count();
    let mut index = 0;
    while !cursor.is_null() && cursor < guns.m_End {
        let gun = unsafe { (*cursor).as_ref() };
        let gun_index = index;
        index += 1;
        cursor = unsafe { cursor.add(1) };
        let Some(gun) = gun else {
            continue;
        };
        let bone = gun
            .m_lBoneAttachementPerWeaponState
            .get(state as usize)
            .copied();
        let Some(hand) = gun_hand(
            weapon as *const WeaponBase as usize,
            state,
            bone,
            gun_index,
            count,
        ) else {
            continue;
        };
        let Some(instance) = (unsafe { gun.m_BaseModelInstance.as_mut() }) else {
            continue;
        };
        let matrices = [
            Mat4::from(gun.m_WorldMatrixT0),
            Mat4::from(gun.m_WorldMatrixT1),
        ];
        let delta = frame.hand_delta(hand, dtf);
        let rendered = delta.map_or_else(
            || lerp_transform(matrices[0], matrices[1], dtf),
            |d| pose_matrix(d) * frame::interpolate_in_character(matrices, dtf),
        );
        if delta.is_some() {
            let matrix = Matrix4::from(rendered);
            unsafe { instance.UpdateMatrix(&matrix) };
            frame::count_weapon_update();
        }
        RENDERED_GUNS.lock()[hand.index()] = Some(RenderedGun {
            weapon: weapon as *const WeaponBase as usize,
            matrix: rendered,
            fraction: dtf,
            at: Instant::now(),
        });
    }
}

static RENDERED_GUNS: Mutex<[Option<RenderedGun>; 2]> = Mutex::new([None; 2]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_vehicle_weapons_keep_both_hands_without_selecting_a_native_slot() {
        let holster = Some(SafeBoneIndex::LEFT_HOLSTER as u32);
        for holder in Hand::BOTH {
            assert_eq!(
                attachment_hand(WeaponState::E_WS_HOLSTERED, holster, Some(holder), 0, 1),
                Some(holder)
            );
            assert_eq!(
                attachment_hand(WeaponState::E_WS_HOLSTERED, holster, Some(holder), 1, 2),
                Some(holder.other())
            );
        }
        assert_eq!(
            attachment_hand(WeaponState::E_WS_HOLSTERED, holster, None, 0, 1),
            None
        );
        assert_eq!(
            attachment_hand(
                WeaponState::E_WS_WIELDING,
                Some(SafeBoneIndex::ATTACH_HAND_RIGHT as u32),
                None,
                0,
                1
            ),
            Some(Hand::Right)
        );
    }

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
