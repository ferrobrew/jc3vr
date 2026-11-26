//! Pause the game's delayed removal of objects the physical hands hold.
//!
//! A detached vehicle part or other object scheduled for removal counts down in the game world
//! object manager's remove queue. A physical hold adds the tick's delta back to the held entries'
//! countdowns before the native update subtracts it, so the object stays until the hands let go.
//! Destruction, streaming, and explicit world removal are not affected.

use std::{num::NonZeroUsize, time::Instant};

use detours_macro::detour;
use jc3gi::game_world_object_manager::GameWorldObjectManager;
use jc3vr_body::controllers::{
    facts::climb::BodyIdentity,
    physics::{bodies, refs::Weak},
    scheme,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
use windows::Win32::System::Threading::{
    CRITICAL_SECTION, EnterCriticalSection, LeaveCriticalSection,
};

use crate::controllers::physical_hands::{climb::engine::body_identity, stale_after};

pub fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_REMOVE_QUEUE_BINDER)
}

/// Publish the bodies the physical hands hold this tick.
///
/// # Safety
/// The game thread, outside the world update.
pub unsafe fn publish(held: [Option<(NonZeroUsize, u32)>; 2]) {
    let now = Instant::now();
    let mut state = STATE.lock();
    state.published = Some(now);
    state.wanted = held;
    let mut previous = std::mem::take(&mut state.proofs);
    state.proofs = held
        .into_iter()
        .flatten()
        .filter_map(|(owner, body)| {
            if let Some(index) = previous
                .iter()
                .position(|proof| proof.owner == owner && proof.body == body)
            {
                return Some(previous.swap_remove(index));
            }
            let identity = unsafe { body_identity(body) }?;
            if identity.owner != owner.get() {
                return None;
            }
            let weak = unsafe { Weak::of(&(*bodies::game_object_of(owner)).m_WeakThis) };
            Some(Proof {
                owner,
                body,
                identity,
                weak,
            })
        })
        .collect();
}

/// Stop protecting the current holds.
pub fn clear() {
    let mut state = STATE.lock();
    state.wanted = [None; 2];
    state.published = None;
    state.proofs.clear();
}

#[detour(address = GameWorldObjectManager::UpdateRemoveQueue_ADDRESS)]
fn update_remove_queue(manager: *mut GameWorldObjectManager, dt: f32) {
    let Some(manager) = (unsafe { manager.as_mut() }) else {
        UPDATE_REMOVE_QUEUE.get().unwrap().call(manager, dt);
        return;
    };
    let proofs = held_proofs(Instant::now());
    let lock = manager.m_RemoveQueueLock;
    if proofs.is_empty() || lock.is_null() || !dt.is_finite() || dt <= 0.0 {
        UPDATE_REMOVE_QUEUE.get().unwrap().call(manager, dt);
        return;
    }
    // The native update takes the same recursive critical section for its own walk.
    let _guard = unsafe { QueueLock::enter(lock.cast::<CRITICAL_SECTION>()) };
    for pending in unsafe { manager.m_RemoveQueue.as_slice_mut() } {
        let held = proofs.iter().any(|(object, control)| {
            pending.m_Object.px as usize == *object && pending.m_Object.pn as usize == *control
        });
        if let Some(paused) = pause(pending.m_TimeLeft, held, dt) {
            pending.m_TimeLeft = paused;
        }
    }
    UPDATE_REMOVE_QUEUE.get().unwrap().call(manager, dt);
}

/// The held objects still in the world, as their game object and control block.
fn held_proofs(now: Instant) -> Vec<(usize, usize)> {
    let state = STATE.lock();
    if !active(&state, now) {
        return Vec::new();
    }
    state
        .proofs
        .iter()
        .filter_map(|proof| {
            if unsafe { body_identity(proof.body) } != Some(proof.identity) {
                return None;
            }
            let strong = proof.weak.lock()?;
            if strong.object() != bodies::game_object_of(proof.owner) {
                return None;
            }
            Some((strong.object() as usize, proof.weak.raw().pn as usize))
        })
        .collect()
}

fn active(state: &State, now: Instant) -> bool {
    state
        .published
        .is_some_and(|at| now.saturating_duration_since(at) <= stale_after())
        && jc3vr_core::hooks::in_gameplay()
        && !jc3vr_core::is_shutting_down()
        && scheme::is_controllers()
        && scheme::enabled()
}

/// A physical hold whose object may be waiting in the remove queue.
struct Proof {
    owner: NonZeroUsize,
    body: u32,
    identity: BodyIdentity,
    weak: Weak,
}

struct State {
    wanted: [Option<(NonZeroUsize, u32)>; 2],
    published: Option<Instant>,
    proofs: Vec<Proof>,
}

static STATE: Mutex<State> = Mutex::new(State {
    wanted: [None; 2],
    published: None,
    proofs: Vec::new(),
});

/// A hold on the game's recursive remove-queue critical section.
struct QueueLock(*mut CRITICAL_SECTION);

impl QueueLock {
    unsafe fn enter(lock: *mut CRITICAL_SECTION) -> Self {
        unsafe { EnterCriticalSection(lock) };
        Self(lock)
    }
}

impl Drop for QueueLock {
    fn drop(&mut self) {
        unsafe { LeaveCriticalSection(self.0) };
    }
}

/// The countdown `remaining` after a held entry's tick is paused, or `None` when it is not held or
/// the values cannot be paused safely.
fn pause(remaining: f32, held: bool, dt: f32) -> Option<f32> {
    let paused = remaining + dt;
    (held
        && remaining.is_finite()
        && remaining > 0.0
        && dt.is_finite()
        && dt > 0.0
        && paused.is_finite())
    .then_some(paused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_countdown_is_restored_while_an_unheld_one_runs() {
        assert_eq!(pause(2.0, true, 0.05), Some(2.05));
        assert_eq!(pause(2.0, false, 0.05), None);
    }

    #[test]
    fn an_invalid_or_spent_countdown_is_not_paused() {
        for remaining in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(pause(remaining, true, 0.05), None);
        }
        for dt in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(pause(2.0, true, dt), None);
        }
        assert_eq!(pause(f32::MAX, true, f32::MAX), None);
    }
}
