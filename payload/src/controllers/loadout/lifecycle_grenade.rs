//! Pure grenade transition plans over the loadout's existing identities and locations.
//!
//! These values are snapshots and proposed commits, not another item table. The future loadout
//! adapter must validate and commit each plan against its one current table. Safe items have no
//! explosive actor. No function here creates an actor, advances a fuse, or changes carried stock.

use std::fmt;

use crate::controllers::{
    frame::Hand,
    loadout::{
        ItemId, LoadoutSnapshot, Place, Socket,
        lifecycle_source::{ActorKey, Generation, RequestId},
        table::Departure,
        transition::Removal,
    },
};

/// The logical grenade state. An active identity does not establish engine liveness.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum GrenadeState {
    Safe,
    Active(ActorKey),
}

/// A view of one item, obtained from the authoritative loadout rather than retained separately.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct GrenadeView {
    pub item: ItemId,
    pub revision: u64,
    pub place: Place,
    pub state: GrenadeState,
}

/// A semantic action. Motion classification belongs to the release-motion/input adapter.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum GrenadeAction {
    MoveToHand(Hand),
    Dock(Socket),
    IncidentalDrop,
    DeliberateThrow,
    Arm,
}

/// A proposed atomic replacement of the same item, never an additional item grant.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct GrenadeCommit {
    pub expected: GrenadeView,
    pub next: GrenadeView,
}

/// A pending activation keeps the safe item and its location unchanged until successful completion.
/// Request identities come from the session's shared non-reusing `RequestIds` allocator.
#[derive(Debug)]
pub(super) struct Activation {
    request: RequestId,
    world: Generation,
    expected: GrenadeView,
    destination: Place,
    phase: ActivationPhase,
}

impl Activation {
    pub fn begin(
        plan: GrenadePlan,
        request: RequestId,
        world: Generation,
    ) -> Result<Self, GrenadeError> {
        let GrenadePlan::Activate {
            expected,
            destination,
        } = plan
        else {
            return Err(GrenadeError::NotActivationPlan);
        };
        Ok(Self {
            request,
            world,
            expected,
            destination,
            phase: ActivationPhase::Pending,
        })
    }

    /// Cancellation does not establish engine cancellation or permit replacement actor requests.
    pub fn cancel(&mut self) {
        if self.phase == ActivationPhase::Pending {
            self.phase = ActivationPhase::Cancelled;
        }
    }

    pub fn outstanding(&self) -> bool {
        !matches!(self.phase, ActivationPhase::Completed(_))
    }

    /// An allocation failure has no actor to dispose and leaves the safe item unchanged.
    pub fn allocation_failed(&mut self, request: RequestId) -> Result<(), GrenadeError> {
        if self.request != request || !self.outstanding() {
            return Err(GrenadeError::StaleActivation);
        }
        self.phase = ActivationPhase::Completed(None);
        Ok(())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum GrenadePlan {
    Commit(GrenadeCommit),
    Activate {
        expected: GrenadeView,
        destination: Place,
    },
}

/// Refusals do not move the item or consume ammunition.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum GrenadeError {
    NotHeld,
    Occupied(Place),
    UnsupportedHolster(Socket),
    ActiveCannotDock,
    AlreadyActive,
    StaleActivation,
    RevisionExhausted,
    NotActivationPlan,
    /// This exact actor was already accepted. A duplicate callback must not dispose it.
    AlreadyAdopted(ActorKey),
}

impl fmt::Display for GrenadeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "loadout grenade: {self:?}")
    }
}

impl std::error::Error for GrenadeError {}

/// Plan against the current occupancy snapshot. The caller serializes validation and commit.
/// A deliberate throw requests activation; an incidental drop never requests activation.
pub(super) fn plan(
    item: GrenadeView,
    action: GrenadeAction,
    occupancy: &LoadoutSnapshot,
) -> Result<GrenadePlan, GrenadeError> {
    let destination = match action {
        GrenadeAction::MoveToHand(hand) => Place::Hand(hand),
        GrenadeAction::Dock(socket) => {
            require_held(item)?;
            if item.state != GrenadeState::Safe {
                return Err(GrenadeError::ActiveCannotDock);
            }
            if !matches!(socket, Socket::Hip(_) | Socket::Chest) || !occupancy.body.has(socket) {
                return Err(GrenadeError::UnsupportedHolster(socket));
            }
            Place::Socket(socket)
        }
        GrenadeAction::IncidentalDrop | GrenadeAction::DeliberateThrow => {
            require_held(item)?;
            Place::World(Departure::Dropped)
        }
        GrenadeAction::Arm => {
            require_held(item)?;
            if item.state != GrenadeState::Safe {
                return Err(GrenadeError::AlreadyActive);
            }
            item.place
        }
    };
    require_available(item.item, destination, occupancy)?;
    if item.state == GrenadeState::Safe
        && matches!(action, GrenadeAction::DeliberateThrow | GrenadeAction::Arm)
    {
        return Ok(GrenadePlan::Activate {
            expected: item,
            destination,
        });
    }
    Ok(GrenadePlan::Commit(commit(item, destination, item.state)?))
}

/// Validate an activation callback before committing its actor identity. Failure leaves the safe
/// item intact. Rejected actors require identity-checked disposal, except `AlreadyAdopted`, which
/// requires ignoring the duplicate return without disposing the adopted actor.
/// Matching callbacks consume the request once, including stale or cancelled returns. An unrelated
/// request callback does not consume this request. Actor disposal is still the adapter's obligation.
pub(super) fn complete_activation(
    pending: &mut Activation,
    request: RequestId,
    world: Generation,
    current: GrenadeView,
    actor: ActorKey,
    occupancy: &LoadoutSnapshot,
) -> Result<GrenadeCommit, GrenadeError> {
    if pending.request == request && pending.phase == ActivationPhase::Completed(Some(actor)) {
        return Err(GrenadeError::AlreadyAdopted(actor));
    }
    if pending.request != request || !pending.outstanding() {
        return Err(GrenadeError::StaleActivation);
    }
    let phase = pending.phase;
    pending.phase = ActivationPhase::Completed(None);
    if phase != ActivationPhase::Pending
        || pending.world != world
        || pending.expected != current
        || current.state != GrenadeState::Safe
    {
        return Err(GrenadeError::StaleActivation);
    }
    require_available(current.item, pending.destination, occupancy)?;
    let committed = commit(current, pending.destination, GrenadeState::Active(actor))?;
    pending.phase = ActivationPhase::Completed(Some(actor));
    Ok(committed)
}

/// A confirmed removal of this exact active identity delegates occupancy cleanup to the existing
/// loadout removal transition. A stale actor callback cannot clear a newer item's binding.
pub(super) fn actor_removed(current: GrenadeView, actor: ActorKey) -> Option<Removal> {
    (current.state == GrenadeState::Active(actor)).then_some(Removal::Item(current.item))
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ActivationPhase {
    Pending,
    Cancelled,
    Completed(Option<ActorKey>),
}

fn require_held(item: GrenadeView) -> Result<(), GrenadeError> {
    if matches!(item.place, Place::Hand(_)) {
        Ok(())
    } else {
        Err(GrenadeError::NotHeld)
    }
}

fn require_available(
    item: ItemId,
    destination: Place,
    occupancy: &LoadoutSnapshot,
) -> Result<(), GrenadeError> {
    if !matches!(destination, Place::World(_))
        && occupancy
            .items
            .iter()
            .any(|other| other.id != item && other.place == destination)
    {
        return Err(GrenadeError::Occupied(destination));
    }
    Ok(())
}

fn commit(
    expected: GrenadeView,
    destination: Place,
    state: GrenadeState,
) -> Result<GrenadeCommit, GrenadeError> {
    let revision = expected
        .revision
        .checked_add(1)
        .ok_or(GrenadeError::RevisionExhausted)?;
    Ok(GrenadeCommit {
        expected,
        next: GrenadeView {
            revision,
            place: destination,
            state,
            ..expected
        },
    })
}

#[cfg(test)]
#[path = "lifecycle_grenade_tests.rs"]
mod tests;
