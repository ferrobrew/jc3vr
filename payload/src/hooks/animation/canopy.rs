//! The canopy's opening out of a grapple reel, skipped for the local player while the head is
//! decoupled.
//!
//! A canopy opened during a reel enters `S_JC_PARACHUTE_OPEN_FROM_REEL_IN_LONG` or
//! `S_JC_PARACHUTE_OPEN_FROM_REEL_IN_SHORT`, whose clips roll Rico over into the harness before the
//! canopy's idle takes over. The player's own body does none of it, and the view, hung from Rico's
//! head through the airborne transitions, rolls with him and passes through his body. The machine
//! is moved on into `S_JC_PARACHUTE_IDLE` before any of the opening's animation is evaluated: the
//! opening is entered first, so its state tasks run (among them the act that opens the canopy
//! object itself), and left before its pose is.
//!
//! Two places catch the opening. The ordinary act transitions (`ACT_TRIGGER_PARACHUTE` here) run
//! through the machine's event handling, which sets the state without calling
//! [`StateMachineInstance::SetStateInternal`], so the local controller's animation update checks
//! the body machine's current state before it evaluates the pose ([`skip_before_evaluation`]).
//! The queued acts' event handling runs in the character's serial state-machine pass, before the
//! pass that updates the animation controller in the same tick, so the check sees an opening entered
//! that tick before any of its pose is evaluated. The controller's update runs as a parallel job,
//! so the check may run on a worker thread. The transitions that do call `SetStateInternal` (`AnimationRuleSystem::SetStateById` and
//! external actions) are caught as they happen.

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

use crate::{
    config::Config,
    hash::hashlittle,
    hooks::{animation::body_rule_system, input::locomotion::head_decoupled},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&SET_STATE_INTERNAL_BINDER)
}

/// How often the canopy's opening out of a reel was skipped. Read by the Air feature's readout.
pub(crate) static REEL_OPENINGS_SKIPPED: AtomicU64 = AtomicU64::new(0);

/// Move the local player's body machine on from a reel opening it is in into the canopy's idle,
/// before the local controller evaluates the pose.
///
/// # Safety
///
/// On the game thread, at the entry of the local player's animation controller update, outside
/// the pose evaluation.
pub(super) unsafe fn skip_before_evaluation() {
    let Some(character) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        return;
    };
    // SAFETY: the local character's body rule system, live on the game thread.
    let Some(machine) = (unsafe { body_rule_system(character) })
        .map(|rule_system| rule_system.m_StateMachineInstance.px)
    else {
        return;
    };
    // SAFETY: the machine is live; its current state is the one the pose is about to evaluate.
    let Some(current) = (unsafe { machine.as_ref() }).map(|machine| machine.m_CurrentState) else {
        return;
    };
    // SAFETY: as above.
    unsafe { skip_reel_opening(machine, current) };
}

#[detour(address = jc3gi::animation::rule_system::StateMachineInstance::SetStateInternal_ADDRESS)]
fn set_state_internal(machine: *mut StateMachineInstance, new_state: *mut State) {
    SET_STATE_INTERNAL.get().unwrap().call(machine, new_state);
    // SAFETY: the game's own arguments, on the thread changing the machine's state.
    unsafe { skip_reel_opening(machine, new_state) };
}

/// Move the local player's body machine on from a reel opening it has entered into the canopy's
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
