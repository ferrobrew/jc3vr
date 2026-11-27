//! The character grabs' engine side: the local player character, a character's place and its
//! `CGameObject` subobject, a character's proxy body, which tells its capsule from its ragdoll's
//! bodies, whether the player's faction is hostile to it, whether it is dead, whether the game
//! protects it, whether its state keeps it out of its ragdoll, and the ragdoll that takes a seated
//! character, such as a mounted gun's gunner, out of their seat.
//!
//! Every function here runs on the game thread outside the physics step.

use glam::{Mat4, Vec3};
use jc3gi::{
    aim::target::FactionRelation, animation::symbol_table::EventIdSymbolTable,
    character::character::Character, faction::GetRelation, game::GameObject,
    physics::PfxCharacterInstance, state::StateBitFlag,
};

use crate::{controllers::aim_state::StateBit, hooks::character::acts::resolve_act_id};

/// `character`'s `CGameObject` subobject, as a weapon's holder names it and the pickups take it
/// as their activator.
#[inline]
pub fn character_object(character: *const Character) -> *mut GameObject {
    character
        .cast::<u8>()
        .wrapping_add(Character::GAME_OBJECT_BASE_OFFSET as usize)
        .cast::<GameObject>()
        .cast_mut()
}

/// `character`'s world transform, as its last simulation tick placed it.
#[inline]
pub(crate) fn character_frame(character: &Character) -> Mat4 {
    Mat4::from(character.m_WorldMatrixT1)
}

/// `character`'s world position: the translation of its world transform ([`character_frame`]).
#[inline]
pub fn character_position(character: &Character) -> Vec3 {
    character_frame(character).w_axis.truncate()
}

/// Why the game keeps a character out of a hand's grab.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Protection {
    /// A mission marks the character as critical to it
    /// ([`Character::m_IsMissionCriticalCharacter`]).
    MissionCritical,
    /// A scripted sequence drives the character ([`Character::m_InSequence`]).
    InSequence,
    /// The character is the player.
    Player,
}

impl Protection {
    pub const fn label(self) -> &'static str {
        match self {
            Protection::MissionCritical => "critical to a mission",
            Protection::InSequence => "in a scripted sequence",
            Protection::Player => "the player",
        }
    }
}

/// The body of `character`'s proxy capsule, or `None` without a proxy.
///
/// # Safety
///
/// `character` must be a live character.
pub unsafe fn proxy_body(character: &Character) -> Option<u32> {
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
#[inline]
pub unsafe fn hostile(character: &Character) -> bool {
    let Some(player) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        return false;
    };
    let relation = unsafe { GetRelation(player.GetFaction(), character.GetFaction()) };
    relation == FactionRelation::E_RELATION_HOSTILE
}

/// Why the game keeps `character` out of a hand's grab, or `None` when nothing does.
///
/// # Safety
///
/// `character` must be a live character.
pub unsafe fn protection(character: &Character) -> Option<Protection> {
    if character.m_IsPlayer {
        Some(Protection::Player)
    } else if character.m_IsMissionCriticalCharacter {
        Some(Protection::MissionCritical)
    } else if character.m_InSequence {
        Some(Protection::InSequence)
    } else {
        None
    }
}

/// Whether `character`'s state skips the hit reaction's ragdoll
/// ([`StateBitFlag::SB_SKIP_HITREACTION_RAGDOLL`]), so that no hit throws it into its ragdoll.
///
/// # Safety
///
/// `character` must be a live character.
pub unsafe fn keeps_footing(character: &Character) -> bool {
    unsafe { state_bit(character, StateBitFlag::SB_SKIP_HITREACTION_RAGDOLL) }
}

/// Why a mounted gun's gunner was not taken out of their seat as a ragdoll.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GunnerOut {
    /// The full ragdoll is queued: the gunner falls out of the seat where they are.
    Queued,
    /// The character does not man a mounted gun in a vehicle.
    NotAGunner,
    /// The game protects the character, it is dead or already ragdolling, or its state skips the
    /// hit reaction's ragdoll.
    Refused,
    /// The seat's state takes no full ragdoll, or the act is not in the symbol table.
    NoTransition,
}

impl GunnerOut {
    pub const fn label(self) -> &'static str {
        match self {
            GunnerOut::Queued => "the full ragdoll is queued",
            GunnerOut::NotAGunner => "not a mounted gun's gunner",
            GunnerOut::Refused => "the character is protected, dead, or keeps its footing",
            GunnerOut::NoTransition => "the seat's state takes no full ragdoll",
        }
    }
}

/// Take `character` out of a mounted gun it mans ([`Character::IsInMountedGunState`], a vehicle's
/// gunner seat or a standalone tripod) as a ragdoll, where it is: the standing path's ragdoll never
/// takes a tripod's gunner off the gun.
///
/// A seated character's hit reaction never ragdolls it ([`Character::HitReaction`] returns before
/// the ragdoll for a character attached to a vehicle), and the vehicle-entry action seats the
/// player rather than removing a gunner. The seat's states take the full-ragdoll act
/// ([`EventIdSymbolTable::ACT_FULL_RAGDOLL`]), whose transition runs the seat task's own exit
/// cleanup; the act is queued only once the seat's state accepts it
/// ([`AnimatedModel::TryAct`](jc3gi::character::character::AnimatedModel::TryAct)).
///
/// # Safety
///
/// `character` must be a live character, on the game thread outside the game's update.
#[inline]
pub unsafe fn gunner_out(character: &mut Character) -> GunnerOut {
    if !unsafe { character.IsInMountedGunState() } {
        return GunnerOut::NotAGunner;
    }
    match unsafe { seat_out(character) } {
        SeatOut::Queued => GunnerOut::Queued,
        SeatOut::Refused => GunnerOut::Refused,
        SeatOut::NoTransition => GunnerOut::NoTransition,
    }
}

/// Whether a seated character was taken out of their seat as a ragdoll.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SeatOut {
    /// The full ragdoll is queued: the character falls out of the seat where they are.
    Queued,
    /// The game protects the character, it is dead or already ragdolling, or its state skips the
    /// hit reaction's ragdoll.
    Refused,
    /// The seat's state takes no full ragdoll, or the act is not in the symbol table.
    NoTransition,
}

impl SeatOut {
    pub const fn label(self) -> &'static str {
        match self {
            SeatOut::Queued => "the full ragdoll is queued",
            SeatOut::Refused => "the character is protected, dead, or keeps its footing",
            SeatOut::NoTransition => "the seat's state takes no full ragdoll",
        }
    }
}

/// Take the seated `character` out of its seat as a ragdoll, where it is, by the full-ragdoll act
/// ([`EventIdSymbolTable::ACT_FULL_RAGDOLL`]), queued only once the seat's state accepts it
/// ([`AnimatedModel::TryAct`](jc3gi::character::character::AnimatedModel::TryAct)). A seated
/// character's hit reaction never ragdolls it ([`Character::HitReaction`]). Which seat states
/// besides a mounted gun's take the act is unverified.
///
/// # Safety
///
/// `character` must be a live character, on the game thread outside the game's update.
pub unsafe fn seat_out(character: &mut Character) -> SeatOut {
    if unsafe { protection(character) }.is_some()
        || unsafe { dead(character) }
        || unsafe { character.IsRagdolling() }
        || unsafe { keeps_footing(character) }
    {
        return SeatOut::Refused;
    }
    let Some(act) = resolve_act_id(EventIdSymbolTable::ACT_FULL_RAGDOLL) else {
        return SeatOut::NoTransition;
    };
    let act = act as u32;
    if !unsafe { character.m_AnimatedModel.TryAct(&act) } {
        return SeatOut::NoTransition;
    }
    unsafe { character.m_AnimatedModel.QueueAct(&act) };
    SeatOut::Queued
}

/// Ragdoll a character a topple left standing, as the hit reaction would have: the hit reaction's
/// ragdoll act ([`EventIdSymbolTable::ACT_HITREACT_RAGDOLL`]) where the state takes it, else the
/// full ragdoll ([`EventIdSymbolTable::ACT_FULL_RAGDOLL`]). The hit reaction returns without
/// ragdolling in states the topple does not see, such as a character holding a gun, which stays
/// standing until it is disarmed. Returns whether an act was
/// queued; a protected, dead, or already ragdolling character, or one whose state keeps its
/// footing, is left alone.
///
/// # Safety
///
/// `character` must be a live character, on the game thread outside the game's update.
pub unsafe fn retry_ragdoll(character: &mut Character) -> bool {
    if unsafe { protection(character) }.is_some()
        || unsafe { dead(character) }
        || unsafe { character.IsRagdolling() }
        || unsafe { keeps_footing(character) }
    {
        return false;
    }
    for name in [
        EventIdSymbolTable::ACT_HITREACT_RAGDOLL,
        EventIdSymbolTable::ACT_FULL_RAGDOLL,
    ] {
        let Some(act) = resolve_act_id(name) else {
            continue;
        };
        let act = act as u32;
        if unsafe { character.m_AnimatedModel.TryAct(&act) } {
            unsafe { character.m_AnimatedModel.QueueAct(&act) };
            return true;
        }
    }
    false
}

/// Whether `character` is getting up out of its ragdoll
/// ([`StateBitFlag::SB_IS_GETTING_UP_FROM_RAGDOLL`]).
///
/// # Safety
///
/// `character` must be a live character.
pub unsafe fn getting_up(character: &Character) -> bool {
    unsafe { state_bit(character, StateBitFlag::SB_IS_GETTING_UP_FROM_RAGDOLL) }
}

/// Whether `character` is dead: no health left, or in the dead state
/// ([`StateBitFlag::SB_IS_IN_DEAD_STATE`]).
///
/// # Safety
///
/// `character` must be a live character.
#[inline]
pub unsafe fn dead(character: &Character) -> bool {
    let health = unsafe { character.GetHealth() };
    health <= 0 || unsafe { state_bit(character, StateBitFlag::SB_IS_IN_DEAD_STATE) }
}

/// Whether `character`'s state has `flag`; `false` before its control parameters exist.
///
/// # Safety
///
/// `character` must be a live character.
unsafe fn state_bit(character: &Character, flag: StateBitFlag) -> bool {
    let Some(control) = (unsafe { character.m_ControlParameters.as_ref() }) else {
        return false;
    };
    let bit = StateBit::of(flag);
    // The three bit-carrying words are adjacent `u64` fields, starting with the first.
    let words = &raw const control.m_UserVM.m_CurrentStateBitFlags0;
    unsafe { words.add(bit.word).read() & bit.mask != 0 }
}
