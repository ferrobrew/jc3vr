//! The character grabs' engine side: a character's proxy body, which tells its capsule from its
//! ragdoll's bodies, and whether the player's faction is hostile to it.
//!
//! Every function here runs on the game thread outside the physics step.

use jc3gi::{
    aim::target::FactionRelation, character::character::Character, faction::GetRelation,
    physics::PfxCharacterInstance,
};

/// The body of `character`'s proxy capsule, or `None` without a proxy.
///
/// # Safety
///
/// `character` must be a live character.
pub(crate) unsafe fn proxy_body(character: &Character) -> Option<u32> {
    let proxy = unsafe { character.m_PfxCharacter.px.as_ref() }?;
    let mut body = PfxCharacterInstance::INVALID_BODY_ID;
    unsafe { proxy.GetCharacterRigidBodyId(&mut body) };
    (body != PfxCharacterInstance::INVALID_BODY_ID).then_some(body)
}

/// Whether the player's faction is hostile to `character`'s ([`GetRelation`]), as the blows judge
/// it. `false` without a player.
///
/// # Safety
///
/// `character` must be a live character.
pub(crate) unsafe fn hostile(character: &Character) -> bool {
    let Some(player) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        return false;
    };
    let relation = unsafe { GetRelation(player.GetFaction(), character.GetFaction()) };
    relation == FactionRelation::E_RELATION_HOSTILE
}
