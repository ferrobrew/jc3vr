//! Re-pushing the held gun's model matrix with the same correction the palette edit applies to the
//! hand bones, so the gun and the hand never separate.
//!
//! The weapon render update runs for every weapon in the world, so it is also where the systems
//! that follow world weapons observe them: each [`ObserveWeapon`] is registered once at install
//! ([`crate::contributions::BodyContributions::weapon_observers`]) and runs before the correction.

use std::ffi::c_void;

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    character::character::SafeBoneIndex,
    equipment::weapon::{WeaponBase, WeaponState},
    game::GameObjectRenderContext,
    types::math::Matrix4,
};
use jc3vr_core::{published::Stamp, render_frame::RenderFrameSlot};
use jc3vr_input::frame::Hand;
use re_utilities::hook_library::HookLibrary;

use crate::{
    contributions,
    controllers::{
        aim::gun,
        hands::{
            ik,
            late::{
                frame,
                math::{lerp_transform, pose_matrix},
            },
        },
        held::{
            self,
            native::{self, weapon_at},
        },
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&WEAPON_UPDATE_RENDER_BINDER)
}

/// One observer's look at a world weapon in its render update.
///
/// # Safety
///
/// The weapon is live for the call, on the thread running its render update; an observer retains
/// only a weak reference to it.
pub type ObserveWeapon = unsafe fn(&WeaponBase);

/// One gun mesh's submitted pose, for comparison with its identified current collision body.
#[derive(Debug, Copy, Clone)]
pub struct RenderedGun {
    pub weapon: usize,
    pub matrix: Mat4,
    pub fraction: f32,
    pub at: Stamp,
}

pub fn rendered_guns() -> [Option<RenderedGun>; 2] {
    RENDERED_GUNS.get().unwrap_or([None; 2])
}

pub(super) fn begin_frame() {
    RENDERED_GUNS.set([None; 2]);
}

/// The hand that owns this gun, retaining paired-gun placement through native state changes.
pub fn gun_hand(
    weapon: usize,
    state: WeaponState,
    bone: Option<u32>,
    index: usize,
    count: usize,
) -> Option<Hand> {
    let owner = held::holder(weapon).or_else(|| held::detonator_hand(weapon));
    attachment_hand(state, bone, owner, index, count)
}

fn attachment_hand(
    state: WeaponState,
    bone: Option<u32>,
    owner: Option<Hand>,
    index: usize,
    count: usize,
) -> Option<Hand> {
    if index >= count {
        return None;
    }
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
pub fn attach_hand(bone: u32) -> Option<Hand> {
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
    let weapon = unsafe { &*weapon_at(game_object.cast()) };
    for observe in
        contributions::get().map_or(&[][..], |contributions| contributions.weapon_observers)
    {
        // SAFETY: this render invocation owns a live native weapon; the observer retains only a
        // weak reference.
        unsafe { observe(weapon) };
    }
    // A finite rack pair's second pistol is drawn apart from the first while neither is claimed.
    if let Some(offset) = held::rack_stack_offset(weapon as *const WeaponBase as usize)
        && let Some(mut gun) = unsafe { native::gun_ptr(weapon, 1) }
    {
        // SAFETY: the gun is its own object, live with the weapon.
        let gun = unsafe { gun.as_mut() };
        let mut rendered = lerp_transform(
            Mat4::from(gun.m_WorldMatrixT0),
            Mat4::from(gun.m_WorldMatrixT1),
            context.m_Dtf,
        );
        rendered.w_axis += offset.extend(0.0);
        if let Some(instance) = unsafe { gun.m_BaseModelInstance.as_mut() } {
            unsafe { instance.UpdateMatrix(&Matrix4::from(rendered)) };
        }
        return;
    }
    let local = ik::local_character();
    // `m_User` is a `weak_ptr<CCharacter>`; its first word is the character. Checked before the
    // frame lock, since this runs for every weapon in the world.
    let address = weapon as *const WeaponBase as usize;
    let owned = held::holder(address).is_some();
    if local == 0 || (weapon.m_User[0] as usize != local && !owned) {
        return;
    }
    let Some(frame) = frame::current() else {
        return;
    };
    let state = weapon.m_State;
    let dtf = context.m_Dtf;
    let count = unsafe { gun::guns(weapon) }.count();
    for gun_index in 0..unsafe { native::gun_count(weapon) } {
        let Some(mut gun) = (unsafe { native::gun_ptr(weapon, gun_index) }) else {
            continue;
        };
        // SAFETY: each gun is its own object, not part of the weapon.
        let gun = unsafe { gun.as_mut() };
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
        let gun = RenderedGun {
            weapon: weapon as *const WeaponBase as usize,
            matrix: rendered,
            fraction: dtf,
            at: Stamp::now(),
        };
        RENDERED_GUNS.update([None; 2], |guns| guns[hand.index()] = Some(gun));
    }
}

/// The guns as the frame's render update placed them, per frame so the draw task's readouts read
/// the frame they draw (see [`jc3vr_core::render_frame`]).
static RENDERED_GUNS: RenderFrameSlot<[Option<RenderedGun>; 2]> = RenderFrameSlot::new();

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
    fn hidden_pistol_meshes_cannot_replace_a_hands_rendered_gun() {
        for holder in Hand::BOTH {
            let mut rendered = [None; 2];
            for index in 0..2 {
                if let Some(hand) = attachment_hand(
                    WeaponState::E_WS_WIELDING,
                    Some(SafeBoneIndex::ATTACH_HAND_LEFT as u32),
                    Some(holder),
                    index,
                    1,
                ) {
                    rendered[hand.index()] = Some(index);
                }
            }
            assert_eq!(rendered[holder.index()], Some(0));
            assert_eq!(rendered[holder.other().index()], None);
            assert_eq!(
                attachment_hand(
                    WeaponState::E_WS_WIELDING,
                    Some(SafeBoneIndex::ATTACH_HAND_LEFT as u32),
                    None,
                    1,
                    1,
                ),
                None
            );
            assert_eq!(
                attachment_hand(WeaponState::E_WS_WIELDING, None, Some(holder), 0, 0),
                None
            );
        }
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
