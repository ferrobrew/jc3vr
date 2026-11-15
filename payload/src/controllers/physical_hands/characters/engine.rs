//! The character grabs' engine side: a character's proxy body, which tells its capsule from its
//! ragdoll's bodies, whether the player's faction is hostile to it, whether it is dead, whether
//! the game protects it, whether its state keeps it out of its ragdoll, and the ragdoll that takes
//! a mounted gun's gunner out of their seat.
//!
//! Every function here runs on the game thread outside the physics step.

use jc3gi::{
    aim::target::FactionRelation, animation::symbol_table::EventIdSymbolTable,
    character::character::Character, faction::GetRelation, physics::PfxCharacterInstance,
    state::StateBitFlag,
};

use crate::{controllers::aim_state::StateBit, hooks::character::acts::resolve_act_id};

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

/// Why the game keeps `character` out of a hand's grab, or `None` when nothing does.
///
/// # Safety
///
/// `character` must be a live character.
pub(crate) unsafe fn protection(character: &Character) -> Option<Protection> {
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
pub(crate) unsafe fn keeps_footing(character: &Character) -> bool {
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
/// gunner seat or a standalone tripod) as a ragdoll, where it is. A tripod's gunner was left out
/// until 15 November, and the standing path's ragdoll never took him off the gun.
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
pub(crate) unsafe fn gunner_out(character: &mut Character) -> GunnerOut {
    if !unsafe { character.IsInMountedGunState() } {
        return GunnerOut::NotAGunner;
    }
    if unsafe { protection(character) }.is_some()
        || unsafe { dead(character) }
        || unsafe { character.IsRagdolling() }
        || unsafe { keeps_footing(character) }
    {
        return GunnerOut::Refused;
    }
    let Some(act) = resolve_act_id(EventIdSymbolTable::ACT_FULL_RAGDOLL) else {
        return GunnerOut::NoTransition;
    };
    let act = act as u32;
    if !unsafe { character.m_AnimatedModel.TryAct(&act) } {
        return GunnerOut::NoTransition;
    }
    unsafe { character.m_AnimatedModel.QueueAct(&act) };
    GunnerOut::Queued
}

/// Whether `character` is dead: no health left, or in the dead state
/// ([`StateBitFlag::SB_IS_IN_DEAD_STATE`]).
///
/// # Safety
///
/// `character` must be a live character.
pub(crate) unsafe fn dead(character: &Character) -> bool {
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
