//! The canopy's opening out of a grapple reel, skipped for the local player while the head is
//! decoupled.
//!
//! A canopy opened during a reel enters `S_JC_PARACHUTE_OPEN_FROM_REEL_IN_LONG` or
//! `S_JC_PARACHUTE_OPEN_FROM_REEL_IN_SHORT`, whose clips roll Rico over into the harness before the
//! canopy's idle takes over. The player's own body does none of it, and the view, hung from Rico's
//! head through the airborne transitions, rolls with him and passes through his body. The machine
//! is moved on into `S_JC_PARACHUTE_IDLE` in the same transition
//! ([`StateMachineInstance::SetStateInternal`]): the opening is entered first, so its state tasks
//! run (among them the act that opens the canopy object itself), and left before any of its
//! animation is evaluated.

use std::sync::{
    LazyLock,
    atomic::{AtomicU64, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    animation::rule_system::{State, StateMachineInstance},
    character::character::Character,
    hash::HashString,
};
use re_utilities::hook_library::HookLibrary;

use crate::{config::Config, hash::hashlittle, hooks::input::locomotion::head_decoupled};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&SET_STATE_INTERNAL_BINDER)
}

/// How often the canopy's opening out of a reel was skipped. Read by the Air feature's readout.
pub(crate) static REEL_OPENINGS_SKIPPED: AtomicU64 = AtomicU64::new(0);

#[detour(address = jc3gi::animation::rule_system::StateMachineInstance::SetStateInternal_ADDRESS)]
fn set_state_internal(machine: *mut StateMachineInstance, new_state: *mut State) {
    SET_STATE_INTERNAL.get().unwrap().call(machine, new_state);
    // SAFETY: the game's own arguments, on the thread changing the machine's state.
    unsafe { skip_reel_opening(machine, new_state) };
}

/// Move the local player's body machine on from a reel opening it just entered into the canopy's
/// idle.
///
/// # Safety
///
/// `machine` and `new_state` are live, on the thread that changes the machine's state.
unsafe fn skip_reel_opening(machine: *mut StateMachineInstance, new_state: *mut State) {
    let Some(state) = (unsafe { new_state.as_ref() }) else {
        return;
    };
    let Some(opening) = REEL_OPENINGS
        .iter()
        .find(|(hash, _)| *hash == state.m_HashID.m_Hash)
        .map(|(_, name)| *name)
    else {
        return;
    };
    if !Config::lock_query(|c| c.controllers.vehicles.canopy.skip_reel_opening) {
        return;
    }
    let Some(character) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() })
        .filter(|character| head_decoupled(character))
    else {
        return;
    };
    // SAFETY: the local character's body rule system, live while its machine changes state.
    let Some(rule_system) = (unsafe { character.m_AnimatedModel.m_RuleSystems.as_slice() })
        .first()
        .and_then(|rule_system| unsafe { rule_system.px.as_mut() })
    else {
        return;
    };
    if !std::ptr::eq(rule_system.m_StateMachineInstance.px, machine) {
        return;
    }
    let idle = HashString {
        m_Hash: *CANOPY_IDLE,
    };
    // SAFETY: as above; the idle is looked up in the machine's own definition.
    if unsafe { rule_system.SetStateById(&raw const idle) } {
        REEL_OPENINGS_SKIPPED.fetch_add(1, Ordering::Relaxed);
        tracing::info!(
            target: "controllers",
            opening,
            "canopy: skipped the opening out of a reel into the canopy's idle"
        );
    } else {
        tracing::warn!(
            target: "controllers",
            opening,
            "canopy: the body machine has no canopy idle to skip the reel opening into"
        );
    }
}

/// The canopy's openings out of a reel, by name hash.
static REEL_OPENINGS: LazyLock<[(u32, &str); 2]> = LazyLock::new(|| {
    [
        "S_JC_PARACHUTE_OPEN_FROM_REEL_IN_LONG",
        "S_JC_PARACHUTE_OPEN_FROM_REEL_IN_SHORT",
    ]
    .map(|name| (hashlittle(name.as_bytes()), name))
});

/// The canopy's idle, by name hash.
static CANOPY_IDLE: LazyLock<u32> = LazyLock::new(|| hashlittle(b"S_JC_PARACHUTE_IDLE"));
