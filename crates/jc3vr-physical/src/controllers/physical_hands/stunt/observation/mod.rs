//! Whether the game's vehicle attachment or stunt traversal owns the player's movement, as the
//! input tick last read it. The reading holds only while it is fresh and its lifecycle context
//! matches the current one; otherwise the owner is unavailable and physical movement yields.

use jc3gi::character::character::Character;
use jc3vr_body::hooks::animation::active_state_type_id;
use jc3vr_core::published::{Stamp, Window};
use parking_lot::Mutex;

use crate::controllers::physical_hands::{
    characters::tuning::CHARACTERS,
    climb::{engine::gate_context, native::Context},
    stunt::engine::PositionRead,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum MovementOwner {
    ObservedKnown(bool),
    Unavailable,
}

impl MovementOwner {
    pub(super) fn yields(self) -> bool {
        self != Self::ObservedKnown(false)
    }
}

pub(super) fn publish(read: Option<PositionRead>, now: Stamp) {
    let context = unsafe { ready_context() };
    *OBSERVATION.lock() = Some(Observation {
        owner: read.map_or(MovementOwner::Unavailable, |r| {
            MovementOwner::ObservedKnown(r.owns_movement)
        }),
        context,
        at: now,
    });
}

pub(super) fn movement_owner() -> MovementOwner {
    let context = unsafe { ready_context() };
    let stale_after = CHARACTERS.get().stunt.observation_stale_after();
    OBSERVATION
        .lock()
        .and_then(|o| o.current(context, Stamp::now(), stale_after))
        .unwrap_or(MovementOwner::Unavailable)
}

/// Admit predicates only after every link they dereference has been checked.
///
/// # Safety
///
/// The character is live on the game thread outside the world update.
pub(super) unsafe fn read_ready<T>(
    character: &Character,
    native: impl FnOnce(&Character) -> T,
) -> Option<T> {
    if character.m_ControlParameters.is_null() || character.m_AnimatedModel.m_InstanceData.is_null()
    {
        return None;
    }
    unsafe { active_state_type_id(character) }?;
    Some(native(character))
}

unsafe fn ready_context() -> Option<Context> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    unsafe { read_ready(character, |_| ()) }?;
    unsafe { gate_context() }
}

#[derive(Copy, Clone)]
struct Observation {
    owner: MovementOwner,
    context: Option<Context>,
    at: Stamp,
}

impl Observation {
    fn current(
        self,
        context: Option<Context>,
        now: Stamp,
        stale_after: Window,
    ) -> Option<MovementOwner> {
        (context.is_some() && context == self.context && stale_after.holds(self.at, now))
            .then_some(self.owner)
    }
}

static OBSERVATION: Mutex<Option<Observation>> = Mutex::new(None);

#[cfg(test)]
mod tests;
