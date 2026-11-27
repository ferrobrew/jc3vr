//! The physical hands' engine side: what a ray hit is, the player's melee impulse and damage, and
//! the body the player stands on.
//!
//! A fast hit's knock is scaled from the player's own melee impulse, so the hammer upgrades carry
//! over; the knock itself is the physics substrate's push
//! ([`jc3vr_body::controllers::physics::bodies::push`]).

use std::num::NonZeroUsize;

use jc3gi::{
    character::{character::Character, player::Player},
    physics::PfxCharacterInstance,
};
use jc3vr_body::controllers::{aim::raycast::RayHit, physics::bodies::is_type};

use crate::controllers::physical_hands::contact::{Surface, SurfaceHit};

/// A ray hit as a palm's surface: a character, or anything else.
///
/// # Safety
///
/// The hit must come from a cast this tick, so that its object is live.
pub(super) unsafe fn surface(hit: &RayHit) -> SurfaceHit {
    let surface = match hit.object {
        Some(object) if unsafe { is_character(object) } => Surface::Character {
            object,
            body: hit.body,
        },
        object => Surface::Solid {
            object,
            body: hit.body,
        },
    };
    SurfaceHit {
        position: hit.position,
        normal: hit.normal,
        surface,
        material: hit.material,
    }
}

/// The player's melee impulse, from the player object behind `character`.
///
/// # Safety
///
/// `character` must be the live local player character.
pub(super) unsafe fn melee_impulse(character: &Character) -> Option<f32> {
    unsafe { player_of(character) }.map(|player| player.m_MeleeImpulse)
}

/// The player's melee damage, from the player object behind `character`.
///
/// # Safety
///
/// `character` must be the live local player character.
pub(super) unsafe fn melee_damage(character: &Character) -> Option<i32> {
    unsafe { player_of(character) }.map(|player| player.m_MeleeDamage)
}

/// The body the player's character proxy stands on, as its last update found it, or `None` without
/// support.
///
/// # Safety
///
/// `character` must be the live local player character.
pub(super) unsafe fn ground_body(character: &Character) -> Option<u32> {
    let proxy = unsafe { character.m_PfxCharacter.px.as_ref() }?;
    (proxy.m_GroundBodyId != PfxCharacterInstance::INVALID_BODY_ID).then_some(proxy.m_GroundBodyId)
}

/// The player object behind the player character `character`.
unsafe fn player_of(character: &Character) -> Option<&Player> {
    if !character.m_IsPlayer || character.m_Avatar.is_null() {
        return None;
    }
    let player = character
        .m_Avatar
        .cast::<u8>()
        .wrapping_sub(Player::AVATAR_BASE_OFFSET as usize)
        .cast::<Player>();
    unsafe { player.as_ref() }
}

/// Whether the physics game object at `object` is a character.
#[inline]
pub(super) unsafe fn is_character(object: NonZeroUsize) -> bool {
    unsafe { is_type(object, Character::TYPE_ID()) }
}
