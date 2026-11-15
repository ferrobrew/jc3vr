//! Standing on a helicopter's skid by its door after a reel onto its door side.
//!
//! A reel onto a helicopter's door side arrives in the game's door-side state, which keeps Rico
//! attached on the skid outside the door and carried by the airframe. Helicopters have no idle
//! there: once the arrival's clip ends, the state drops into the helicopter's stunt hang through
//! [`ValidateStuntPositionCondition::Evaluate`] on the transition to the side-to-hang state, unless
//! the player pressed Y in time to enter. The reviewer wants the rail as a place to stand: so while
//! the controllers act, that one transition is refused, and the door-side state, with no other way
//! out at its clip's end, holds Rico on the skid. Y still enters through the state's own
//! transitions.
//!
//! Stepping off is a fall without a jump: the jump press takes Rico out of the helicopter's
//! interaction graph as the game takes a character off a car's roof when they walk past its stunt
//! area's edge ([`InteractionGraph::RemoveUser`], the user's motion state back on, its graph state
//! inactive, and the physics input's game-controlled velocity off), and gives him the velocity he
//! moved at with the airframe. The door-side state then falls on its next update, as a character
//! no longer in a vehicle does.
//!
//! [`ValidateStuntPositionCondition::Evaluate`]: jc3gi::animation::rule_system::ValidateStuntPositionCondition::Evaluate
//! [`InteractionGraph::RemoveUser`]: jc3gi::interaction::InteractionGraph::RemoveUser

use std::sync::{
    LazyLock,
    atomic::{AtomicU64, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    animation::rule_system::{StateMachineInstance, Transition, ValidateStuntPositionCondition},
    character::character::Character,
    types::math::Vector3,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::scheme,
    hash::hashlittle,
    hooks::animation::{active_state_type_id, body_rule_system},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&EVALUATE_BINDER)
}

/// How often the drop from the door side into the stunt hang was refused, for the debug UI.
pub fn drops_refused() -> u64 {
    DROPS_REFUSED.load(Ordering::Relaxed)
}

/// Step the local player off the helicopter's skid when `jump` was pressed while the rail holds
/// him: out of the helicopter's interaction graph, falling with the velocity he moved at.
///
/// # Safety
///
/// The game stage, on the game thread before the world update.
pub(crate) unsafe fn step_off_on_jump(jump: bool) {
    if !jump || !enabled() {
        return;
    }
    let Some(character) = (unsafe { Character::GetLocalPlayerCharacter().as_mut() }) else {
        return;
    };
    if !unsafe { on_door_side(character) } {
        return;
    }
    let velocity = character.m_MeasuredWorldVelocity;
    let proxy = &raw mut character.m_InteractionUserProxy;
    // SAFETY: the proxy is the character's own, live with it; the graph is live while it has the
    // user.
    unsafe {
        if let Some(graph) = (*proxy).m_Graph.as_mut() {
            graph.RemoveUser(proxy);
        }
        (*proxy).m_MotionStateEnabled = true;
        (*proxy).SetGraphUserState(GRAPH_USER_INACTIVE);
        if let Some(pfx) = character.m_PfxCharacter.px.as_mut() {
            pfx.m_Input.gameControledVelocity = false;
        }
        let velocity = Vector3 {
            data: velocity.data,
        };
        character.SetWantedWorldVelocity(&velocity);
    }
    tracing::info!(target: "controllers", "rail: stepped off the helicopter's skid");
}

#[detour(address = jc3gi::animation::rule_system::ValidateStuntPositionCondition::Evaluate_ADDRESS)]
fn evaluate(
    condition: *const ValidateStuntPositionCondition,
    machine: *const StateMachineInstance,
    transition: *const Transition,
) -> bool {
    // SAFETY: the state machine evaluates its own transition's condition on its own thread.
    if enabled() && unsafe { drops_to_hang(machine, transition) } {
        if DROPS_REFUSED.fetch_add(1, Ordering::Relaxed) == 0 {
            tracing::info!(
                target: "controllers",
                "rail: holding Rico on the helicopter's skid rather than dropping into the hang"
            );
        }
        return false;
    }
    EVALUATE.get().unwrap().call(condition, machine, transition)
}

/// Whether the rail acts: the controllers' scheme, and its switch on.
fn enabled() -> bool {
    scheme::is_controllers()
        && Config::lock_query(|c| c.controllers.enabled && c.controllers.vehicles.helicopter_rail)
}

/// Whether `transition` of `machine` is the local player's drop from a helicopter's door side into
/// its stunt hang.
///
/// # Safety
///
/// The machine and the transition are live for the condition's evaluation.
unsafe fn drops_to_hang(
    machine: *const StateMachineInstance,
    transition: *const Transition,
) -> bool {
    let Some(character) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        return false;
    };
    let local_machine = unsafe { body_rule_system(character) }
        .is_some_and(|rules| std::ptr::eq(rules.m_StateMachineInstance.px.cast_const(), machine));
    if !local_machine {
        return false;
    }
    let Some(destination) = (unsafe { transition.as_ref() })
        .and_then(|transition| unsafe { transition.GetDestination().as_ref() })
    else {
        return false;
    };
    TO_HANG.contains(&destination.m_HashID.m_Hash)
}

/// Whether `character` is in a helicopter's door-side state, where the rail holds him.
///
/// # Safety
///
/// `character` is live, on the game thread.
unsafe fn on_door_side(character: &Character) -> bool {
    unsafe { active_state_type_id(character) }.is_some_and(|state| DOOR_SIDE.contains(&state))
}

/// The states a helicopter's door side drops into the stunt hang through.
static TO_HANG: LazyLock<[u32; 2]> = LazyLock::new(|| {
    [
        hashlittle(b"S_DRIVER_SIDE_TO_HANG_VEHICLE"),
        hashlittle(b"S_PASSENGER_SIDE_TO_HANG_VEHICLE"),
    ]
});

/// The door-side states a reel onto a helicopter arrives in.
static DOOR_SIDE: LazyLock<[u32; 2]> = LazyLock::new(|| {
    [
        hashlittle(b"S_GRAPPLE_REEL_TO_HELI_DRIVER_SIDE"),
        hashlittle(b"S_GRAPPLE_REEL_TO_HELI_PASSENGER_SIDE"),
    ]
});

/// The interaction graph user's inactive state.
const GRAPH_USER_INACTIVE: i32 = 0;

static DROPS_REFUSED: AtomicU64 = AtomicU64::new(0);
