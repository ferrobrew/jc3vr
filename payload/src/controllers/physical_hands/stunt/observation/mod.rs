//! Checked native movement observation and its bounded input-tick publication.

use std::time::{Duration, Instant};

use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::{
    controllers::physical_hands::{
        climb::{engine::gate_context, native::Context},
        stunt::engine::PositionRead,
    },
    hooks::animation::active_state_type_id,
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

pub(super) fn publish(read: Option<PositionRead>, now: Instant) {
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
    OBSERVATION
        .lock()
        .and_then(|o| o.current(context, Instant::now()))
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
    at: Instant,
}

impl Observation {
    fn current(self, context: Option<Context>, now: Instant) -> Option<MovementOwner> {
        (context.is_some()
            && context == self.context
            && now.saturating_duration_since(self.at) <= STALE_AFTER)
            .then_some(self.owner)
    }
}

const STALE_AFTER: Duration = Duration::from_millis(200);
static OBSERVATION: Mutex<Option<Observation>> = Mutex::new(None);

#[cfg(test)]
mod tests;
