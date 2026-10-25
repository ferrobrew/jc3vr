//! Owned whole-group callback obligations for the controlled transport.
//!
//! There is no native implementation. A future wrapper must retain every object, including
//! non-weapons, before the borrowed callback vector expires. It must neither copy `SharedPtr`
//! bits nor perform reference operations under a mod-state lock.

use std::num::NonZeroU64;

use crate::controllers::loadout::lifecycle_source::{RequestId, ReturnedActor};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Epoch(pub u64);

/// A retained native group's identity, not an actor address or a spawn tag.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct GroupId(pub NonZeroU64);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Ticket {
    pub request: RequestId,
    pub epoch: Epoch,
}

/// The complete group remains inseparable, even when none of its objects is a weapon.
#[derive(Debug)]
pub(super) struct OwnedGroup<R> {
    pub id: GroupId,
    pub ticket: Ticket,
    pub objects: Vec<R>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum CleanupProgress {
    /// Native ownership transferred for the complete group; release waits for the service return.
    Queued { objects: usize },
    /// A zero or otherwise unclassified result does not prove disposal or membership transfer.
    Indeterminate,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum ReleaseReason {
    CleanupBoundary,
    DuplicateReference,
}

/// A typed, synchronous seam for a future verified owning wrapper, not an engine binding.
/// Calls occur outside callbacks and without a mod-state lock. `cleanup_group` must operate on
/// the complete owned group, never on a member selected from a multi-object callback.
pub(super) trait Engine {
    type Strong;

    fn spawn(&mut self, ticket: Ticket);
    fn cancel(&mut self, ticket: Ticket);
    fn cleanup_group(&mut self, group: &OwnedGroup<Self::Strong>) -> CleanupProgress;
    fn release_group(&mut self, group: OwnedGroup<Self::Strong>, reason: ReleaseReason);
}

/// A callback borrow cannot be converted into owned transport data by copying pointer bits.
/// This trait deliberately has no native implementation. Retention is all-or-nothing: an
/// implementation cannot return success after retaining only the weapon subset of a group.
pub(super) trait BorrowedGroup {
    type Strong;

    fn retain_all(&self) -> OwnedGroup<Self::Strong>;
    fn classify(&self) -> Option<Vec<ReturnedActor>>;
}

/// A rejected handoff still owns every returned object. It must remain alive until a verified
/// cleanup route or a caller-owned bounded retirement slot accepts the entire obligation.
#[must_use = "a callback refusal still owns its complete group"]
#[derive(Debug)]
pub(super) struct Callback<R> {
    pub group: OwnedGroup<R>,
    pub classified: Option<Vec<ReturnedActor>>,
}

impl<R> Callback<R> {
    pub fn retain(borrowed: &impl BorrowedGroup<Strong = R>) -> Self {
        Self {
            group: borrowed.retain_all(),
            classified: borrowed.classify(),
        }
    }
}
