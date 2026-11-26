//! AI visibility of owned inert allocations. Only the game thread resolves the weak actors.
//! The awareness detour reads allocation identities from a separate snapshot.

use detours_macro::detour;
use jc3gi::{
    ai::ai::{AwarenessRecord, SenseAwareness},
    equipment::explosive::Explosive,
};
use parking_lot::{Mutex, RwLock};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::weapons::grenade::native::{Actor, inert_fuse, pin_out};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GRENADE_CHECK_BINDER)
}

/// The pool slot and its allocation generation, independent of the actor's address.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Allocation {
    pool: u32,
    instance: u32,
}

impl Allocation {
    pub(super) fn of(explosive: &Explosive) -> Self {
        Self {
            pool: explosive.m_PoolIndex,
            instance: explosive.m_InstanceUniqueId,
        }
    }
}

/// Publish only actors created by the safe grenade factory, after their fuse is made inert.
pub(super) fn publish(actor: &Actor) {
    let mut owned = OWNED.lock();
    owned.insert(actor.allocation, actor.clone());
    *SNAPSHOT.write() = owned.snapshot();
}

/// Stop suppressing awareness before starting a fuse or removing an actor from the pool.
pub(super) fn remove(actor: &Actor) {
    let mut owned = OWNED.lock();
    owned.remove(actor.allocation);
    *SNAPSHOT.write() = owned.snapshot();
}

/// Reconcile weak handles without consulting loadout membership or the pending release queue.
/// Actors awaiting release or cleanup remain suppressed for as long as they remain inert.
///
/// # Safety
///
/// The caller runs at the serialized game-thread boundary.
pub(super) unsafe fn reconcile() {
    let mut owned = OWNED.lock();
    owned.reconcile(|actor| {
        let explosive = actor.explosive()?;
        // SAFETY: the weak handle and allocation generation match on the game thread.
        let explosive = unsafe { &*explosive };
        (!pin_out(actor)
            && inert_fuse(
                explosive.m_StartDetonationTimer,
                explosive.m_DetonationTimer,
            ))
        .then_some(actor.allocation)
    });
    *SNAPSHOT.write() = owned.snapshot();
}

#[detour(address = SenseAwareness::GrenadeCheck_ADDRESS)]
fn grenade_check(sense: *const SenseAwareness, event: *const AwarenessRecord) -> bool {
    // SAFETY: the engine supplies the awareness record for the duration of this call.
    // No actor, weak handle, or ammunition-pool pointer is read on the AI thread.
    if let Some(event) = unsafe { event.as_ref() } {
        let allocation = Allocation {
            pool: event.m_GrenadePoolIndex,
            instance: event.m_GrenadeInstanceUniqueId,
        };
        if SNAPSHOT.read().contains(&allocation) {
            return false;
        }
    }
    GRENADE_CHECK.get().unwrap().call(sense, event)
}

struct Owned<A> {
    entries: Vec<(Allocation, A)>,
}

impl<A> Owned<A> {
    fn insert(&mut self, allocation: Allocation, actor: A) {
        // A pooled object can retain its control block across allocation reuse.
        self.entries.retain(|(key, _)| key.pool != allocation.pool);
        self.entries.push((allocation, actor));
    }

    fn remove(&mut self, allocation: Allocation) {
        self.entries.retain(|(key, _)| *key != allocation);
    }

    fn reconcile(&mut self, current: impl Fn(&A) -> Option<Allocation>) {
        self.entries
            .retain(|(allocation, actor)| current(actor) == Some(*allocation));
    }

    fn snapshot(&self) -> Vec<Allocation> {
        self.entries
            .iter()
            .map(|(allocation, _)| *allocation)
            .collect()
    }
}

// The registry owns weak handles until arming, pool removal, or game-thread reconciliation.
// Clearing the release queue or moving actors into deferred cleanup must not clear this snapshot.
static OWNED: Mutex<Owned<Actor>> = Mutex::new(Owned {
    entries: Vec::new(),
});
static SNAPSHOT: RwLock<Vec<Allocation>> = RwLock::new(Vec::new());

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: Allocation = Allocation {
        pool: 7,
        instance: 31,
    };
    const REUSED: Allocation = Allocation {
        pool: FIRST.pool,
        instance: FIRST.instance + 1,
    };
    const OTHER: Allocation = Allocation {
        pool: FIRST.pool + 1,
        instance: FIRST.instance,
    };

    fn owned() -> Owned<Option<Allocation>> {
        Owned {
            entries: Vec::new(),
        }
    }

    #[test]
    fn only_matching_owned_allocation_is_suppressed() {
        let mut owned = owned();
        assert!(!owned.snapshot().contains(&FIRST));
        owned.insert(FIRST, Some(FIRST));
        let snapshot = owned.snapshot();
        assert!(snapshot.contains(&FIRST));
        assert!(!snapshot.contains(&REUSED));
        assert!(!snapshot.contains(&OTHER));
    }

    #[test]
    fn reused_slot_replaces_generation_and_old_removal_cannot_remove_it() {
        let mut owned = owned();
        owned.insert(FIRST, Some(FIRST));
        owned.insert(REUSED, Some(REUSED));
        owned.remove(FIRST);
        assert_eq!(owned.snapshot(), vec![REUSED]);
        owned.remove(REUSED);
        assert!(owned.snapshot().is_empty());
    }

    #[test]
    fn repeated_publication_is_idempotent() {
        let mut owned = owned();
        owned.insert(FIRST, Some(FIRST));
        owned.insert(FIRST, Some(FIRST));
        assert_eq!(owned.snapshot(), vec![FIRST]);
    }

    #[test]
    fn reconciliation_forgets_expired_live_and_recycled_actors() {
        let mut owned = owned();
        owned.insert(FIRST, Some(REUSED));
        owned.insert(OTHER, None);
        owned.reconcile(|current| *current);
        assert!(owned.snapshot().is_empty());
    }

    #[test]
    fn unrelated_actor_survives_arming_or_removal() {
        let mut owned = owned();
        owned.insert(FIRST, Some(FIRST));
        owned.insert(OTHER, Some(OTHER));
        owned.remove(FIRST);
        owned.reconcile(|current| *current);
        assert_eq!(owned.snapshot(), vec![OTHER]);
    }

    #[test]
    fn pending_release_and_cleanup_keep_live_inert_membership() {
        let mut owned = owned();
        owned.insert(FIRST, Some(FIRST));
        let mut pending_release = vec![FIRST];
        let mut loadout = vec![FIRST];
        pending_release.clear();
        let retiring = std::mem::take(&mut loadout);
        owned.reconcile(|current| *current);
        assert_eq!(owned.snapshot(), retiring);
        for allocation in retiring {
            owned.remove(allocation);
        }
        assert!(owned.snapshot().is_empty());
    }

    #[test]
    fn weak_handle_that_survives_pool_reuse_does_not_retain_old_membership() {
        let mut owned = owned();
        owned.insert(FIRST, Some(FIRST));
        owned.entries[0].1 = Some(REUSED);
        owned.reconcile(|current| *current);
        assert!(owned.snapshot().is_empty());
    }
}
