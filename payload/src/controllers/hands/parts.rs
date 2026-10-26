//! The render-time late update of a held gun's separate model parts: the magazine, the slide, or the
//! spinning barrel that a weapon draws as its own model next to the gun's
//! ([`ModelAttachementWeaponComponent`], and the barrel recoil and spin components derived from it).
//!
//! A part keeps its own cached world matrices. Its local transforms relative to the owning gun
//! interpolate beneath that gun's rendered parent, preserving mechanical travel through a vehicle
//! turn. A post-call render detour applies the same hand correction as the gun's late update
//! ([`crate::controllers::hands::late`]) for held and wielded weapons.
//!
//! A part rides its gun, and so the hand the gun's attach bone belongs to, unless an animation
//! task put it on one of the holder's bones for the placement (a magazine carried in the off-hand
//! during a reload). That bone is reset at the end of the pass that placed the part, before the
//! render update can see it, so a post-call detour on the part's placement records it for the
//! local player's weapons.
//!
//! The true reload moves two kinds of part on top of the correction
//! ([`crate::controllers::weapons::reload`]): a pistol's slide, held back while its chamber is
//! empty and following the hand while it is worked, and a magazine part carried in the hand that
//! took it from the pouch.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    character::character::SafeBoneIndex,
    equipment::weapon::{ModelAttachementWeaponComponent, WeaponBase, WeaponData},
    game::GameObjectRenderContext,
    types::math::Matrix4,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::{ik, late},
    weapons::{
        hold::{self, armed},
        reload,
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&PART_UPDATE_ATTACHMENT_TRANSFORMS_BINDER)
        .with_static_binder(&PART_UPDATE_RENDER_BINDER)
}

/// Model parts re-pushed since the payload loaded.
pub fn parts_updated() -> u64 {
    PARTS_UPDATED.load(Ordering::Relaxed)
}

/// The hand a part rides: the hand of the holder's bone it was last placed on, or, while it rides
/// its gun (`placed_on` is [`ModelAttachementWeaponComponent::NO_CHARACTER_BONE`]), the hand of the
/// gun's attach bone `gun_bone`. `None` for a part on a holster or any other bone off the hands.
pub fn part_hand(placed_on: u32, gun_bone: Option<u32>) -> Option<Hand> {
    if placed_on == ModelAttachementWeaponComponent::NO_CHARACTER_BONE {
        return gun_bone.and_then(late::attach_hand);
    }
    late::attach_hand(placed_on).or_else(|| {
        [
            (SafeBoneIndex::LEFT_HAND as u32, Hand::Left),
            (SafeBoneIndex::RIGHT_HAND as u32, Hand::Right),
        ]
        .into_iter()
        .find(|&(bone, _)| bone == placed_on)
        .map(|(_, hand)| hand)
    })
}

/// The bone each of the local player's parts was last placed on, by the part's address. A part
/// that has not been placed since the payload loaded counts as riding its gun.
static PLACED_ON: Mutex<Option<HashMap<usize, u32>>> = Mutex::new(None);
/// More entries than the local player's weapons carry parts; past it the record is rebuilt, which
/// drops the parts of weapons since freed.
const MAX_RECORDED_PARTS: usize = 64;
static PARTS_UPDATED: AtomicU64 = AtomicU64::new(0);

/// The part's placement, then the bone it was placed on recorded for a local player's weapon.
#[detour(
    address = jc3gi::equipment::weapon::ModelAttachementWeaponComponent::UpdateAttachmentTransforms_ADDRESS
)]
fn part_update_attachment_transforms(
    part: *mut ModelAttachementWeaponComponent,
    snow_factor: f32,
    water_factor: f32,
    transform_type: u32,
) {
    PART_UPDATE_ATTACHMENT_TRANSFORMS.get().unwrap().call(
        part,
        snow_factor,
        water_factor,
        transform_type,
    );
    // Every character's weapon parts are placed here on the simulation's worker threads, so the
    // local player's are picked out before any lock.
    let Some(part) = (unsafe { part.as_ref() }) else {
        return;
    };
    if !unsafe { part.m_Owner.as_ref() }.is_some_and(|w| {
        let address = w as *const WeaponBase as usize;
        w.m_IsUserLocalPlayer || hold::owned_holder(address).is_some() || armed::is(address)
    }) {
        return;
    }
    let mut placed = PLACED_ON.lock();
    let placed = placed.get_or_insert_with(HashMap::new);
    if placed.len() >= MAX_RECORDED_PARTS {
        placed.clear();
    }
    placed.insert(part as *const _ as usize, part.m_CharacterBone);
}

/// The part's render update, then its model matrix re-pushed with its hand's correction.
#[detour(address = jc3gi::equipment::weapon::ModelAttachementWeaponComponent::UpdateRender_ADDRESS)]
fn part_update_render(
    part: *mut ModelAttachementWeaponComponent,
    context: *const GameObjectRenderContext,
) {
    PART_UPDATE_RENDER.get().unwrap().call(part, context);
    unsafe { late_update_part(part, context) };
}

/// Re-push `part`'s model matrix with the correction of the hand it rides, when its weapon is
/// wielded by the local player.
unsafe fn late_update_part(
    part: *mut ModelAttachementWeaponComponent,
    context: *const GameObjectRenderContext,
) {
    let (Some(part), Some(context)) = (unsafe { part.as_ref() }, unsafe { context.as_ref() })
    else {
        return;
    };
    let Some(weapon) = (unsafe { part.m_Owner.as_ref() }) else {
        return;
    };
    let local = ik::local_character();
    // `m_User` is a `weak_ptr<CCharacter>`; its first word is the character.
    let address = weapon as *const WeaponBase as usize;
    let owned = hold::owned_holder(address)
        .or_else(|| armed::hand_of(address))
        .is_some();
    if local == 0 || (weapon.m_User[0] as usize != local && !owned) {
        return;
    }
    let state = weapon.m_State;
    let placed_on = PLACED_ON
        .lock()
        .as_ref()
        .and_then(|p| p.get(&(part as *const _ as usize)).copied())
        .unwrap_or(ModelAttachementWeaponComponent::NO_CHARACTER_BONE);
    let Some(gun) = (unsafe { part_gun(weapon, part.m_WeaponDataIndex) }) else {
        return;
    };
    let gun_bone = gun
        .m_lBoneAttachementPerWeaponState
        .get(state as usize)
        .copied();
    let Some(holder) = late::gun_hand(
        weapon as *const WeaponBase as usize,
        state,
        gun_bone,
        part.m_WeaponDataIndex as usize,
        unsafe { gun::guns(weapon) }.count(),
    ) else {
        return;
    };
    let hand = if placed_on == ModelAttachementWeaponComponent::NO_CHARACTER_BONE {
        holder
    } else if let Some(hand) = part_hand(placed_on, gun_bone) {
        hand
    } else {
        return;
    };
    let dtf = context.m_Dtf;
    let Some(instance) = (unsafe { part.m_BaseModelInstance.as_mut() }) else {
        return;
    };
    let matrices = [
        Mat4::from(part.m_WorldMatrixSaved),
        Mat4::from(part.m_WorldMatrix),
    ];
    let on_gun = placed_on == ModelAttachementWeaponComponent::NO_CHARACTER_BONE;
    let correction = late::hand_correction(hand, dtf);
    let interpolated = if on_gun {
        let parent = [
            Mat4::from(gun.m_WorldMatrixT0),
            Mat4::from(gun.m_WorldMatrixT1),
        ];
        let rendered = if correction.is_some() {
            late::interpolate_in_character(parent, dtf)
        } else {
            late::lerp_transform(parent[0], parent[1], dtf)
        };
        animated_part(parent, matrices, rendered, dtf)
    } else {
        late::lerp_transform(matrices[0], matrices[1], dtf)
    };
    let corrected = correction.map_or(interpolated, |d| d * interpolated);
    let adjusted = reload::adjust_part(part, weapon as *const WeaponBase as usize, corrected, dtf);
    if !on_gun && correction.is_none() && adjusted.is_none() {
        return;
    }
    let matrix = Matrix4::from(adjusted.unwrap_or(corrected));
    unsafe { instance.UpdateMatrix(&matrix) };
    PARTS_UPDATED.fetch_add(1, Ordering::Relaxed);
}

fn animated_part(parent: [Mat4; 2], part: [Mat4; 2], rendered_parent: Mat4, dtf: f32) -> Mat4 {
    let local = [parent[0].inverse() * part[0], parent[1].inverse() * part[1]];
    if local.iter().all(|m| m.is_finite()) {
        rendered_parent * late::lerp_transform(local[0], local[1], dtf)
    } else {
        late::lerp_transform(part[0], part[1], dtf)
    }
}

/// The gun that owns this part, not merely the selected weapon's first gun.
unsafe fn part_gun(weapon: &WeaponBase, index: u32) -> Option<&WeaponData> {
    unsafe { gun::guns(weapon) }
        .nth(usize::try_from(index).ok()?)
        .flatten()
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec3};

    use super::*;

    #[test]
    fn vehicle_translation_and_turn_preserve_the_animated_action_offset() {
        let local = [
            Mat4::from_translation(Vec3::Z * 0.1),
            Mat4::from_translation(Vec3::Z * 0.16),
        ];
        for (distance, yaw) in [(0.0, 0.0), (4.0, 0.0), (2.0, 0.8), (-1.0, -0.4)] {
            let parent = [
                Mat4::from_translation(Vec3::new(100.0, 3.0, -50.0)),
                Mat4::from_rotation_translation(
                    Quat::from_rotation_y(yaw),
                    Vec3::new(100.0 + distance, 3.0, -50.0),
                ),
            ];
            let part = [parent[0] * local[0], parent[1] * local[1]];
            for t in [0.0, 0.25, 0.5, 1.0] {
                let rendered = late::lerp_transform(parent[0], parent[1], t);
                let actual = animated_part(parent, part, rendered, t);
                let action = rendered.inverse() * actual;
                assert!(action.abs_diff_eq(late::lerp_transform(local[0], local[1], t), 2e-5));
            }
        }
    }

    #[test]
    fn a_part_follows_the_late_parent_without_losing_mechanical_travel() {
        let parent = [Mat4::IDENTITY; 2];
        let parts = [Mat4::IDENTITY, Mat4::from_translation(Vec3::Z * 0.08)];
        let late_parent = Mat4::from_rotation_translation(Quat::from_rotation_x(0.3), Vec3::X);
        let result = animated_part(parent, parts, late_parent, 0.5);
        assert!(
            (late_parent.inverse() * result)
                .abs_diff_eq(Mat4::from_translation(Vec3::Z * 0.04), 1e-5)
        );
    }

    const ON_GUN: u32 = ModelAttachementWeaponComponent::NO_CHARACTER_BONE;

    #[test]
    fn a_part_on_its_gun_rides_the_gun_hand() {
        assert_eq!(
            part_hand(ON_GUN, Some(SafeBoneIndex::ATTACH_HAND_RIGHT as u32)),
            Some(Hand::Right)
        );
        assert_eq!(
            part_hand(ON_GUN, Some(SafeBoneIndex::ATTACH_HAND_LEFT as u32)),
            Some(Hand::Left)
        );
        assert_eq!(
            part_hand(ON_GUN, Some(SafeBoneIndex::LEFT_HOLSTER as u32)),
            None
        );
        assert_eq!(part_hand(ON_GUN, None), None);
    }

    #[test]
    fn a_part_on_a_holder_bone_rides_that_bone_hand() {
        let gun = Some(SafeBoneIndex::ATTACH_HAND_RIGHT as u32);
        assert_eq!(
            part_hand(SafeBoneIndex::LEFT_HAND as u32, gun),
            Some(Hand::Left)
        );
        assert_eq!(
            part_hand(SafeBoneIndex::ATTACH_HAND_LEFT2 as u32, gun),
            Some(Hand::Left)
        );
        assert_eq!(part_hand(SafeBoneIndex::SPINE2 as u32, gun), None);
    }
}
