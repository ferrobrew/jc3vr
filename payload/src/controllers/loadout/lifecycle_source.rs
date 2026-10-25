//! Bounded pure bookkeeping for locker sources, member claims, and request completion.
//!
//! Source keys come from a future verified provider, not from a weapon tag or address alone.
//! Returned actor keys are logical identities supplied by that provider, not owned engine handles.
//! Completion receipts classify every return; acknowledging a receipt records the adapter's claim
//! that its required disposal work is complete. This module cannot establish engine disposal.
//! Carried items and occupancy remain in the existing loadout. Pair callbacks stage at the source
//! and never reserve a hand or automatically equip the other member.

use std::{fmt, num::NonZeroU64};

use crate::controllers::{
    frame::Hand,
    loadout::{LoadoutSnapshot, ObjectAddress, WeaponType, lifecycle_ammo::GunAmmo},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Generation(pub u64);

/// The provider must distinguish a reused source identity and a new world.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct SourceKey {
    pub identity: NonZeroU64,
    pub world: Generation,
    pub incarnation: Generation,
}

/// A provider-validated actor identity. Address equality alone is insufficient.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct ActorKey {
    pub address: ObjectAddress,
    pub generation: Generation,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct RequestId(NonZeroU64);

/// Shared by source preparation and grenade activation for one session. It refuses exhaustion.
#[derive(Debug)]
pub(super) struct RequestIds {
    next: Option<NonZeroU64>,
}

impl Default for RequestIds {
    fn default() -> Self {
        Self {
            next: Some(NonZeroU64::MIN),
        }
    }
}

impl RequestIds {
    pub fn issue(&mut self) -> Result<RequestId, SourceError> {
        let id = self.next.ok_or(SourceError::IdentityExhausted)?;
        self.next = id.checked_add(1);
        Ok(RequestId(id))
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Member {
    First,
    Second,
}

/// Qualification and per-member ammunition allocations are supplied by verified source inspection.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Offering {
    UnlimitedGrenades {
        kind: u32,
    },
    Single {
        weapon: WeaponType,
        ammo: GunAmmo,
    },
    Pair {
        weapon: WeaponType,
        ammo: [GunAmmo; 2],
    },
}

/// A safe draw has no actor, reserve count, or refundable inventory grant. The existing table's
/// allocator supplies its ItemId when an adapter can atomically admit the actor-free item.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SafeDraw {
    pub source: SourceKey,
    pub kind: u32,
    pub hand: Hand,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct ReturnedActor {
    pub actor: ActorKey,
    pub weapon: WeaponType,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Disposition {
    Staged(ActorKey),
    Dispose(ActorKey),
    /// A repeated entry in this callback batch carries no additional actor ownership.
    Repeated(ActorKey),
    /// A duplicate callback must not dispose an actor now staged or carried elsewhere.
    AlreadyOwned(ActorKey),
    /// A previous unacknowledged batch already owns this disposal obligation.
    CleanupPending(ActorKey),
    /// This address has a different current identity. Do not dispose through the stale key.
    IdentityConflict(ActorKey),
}

/// A non-clone receipt for an admitted callback batch. All disposals must complete before
/// acknowledgment. A rejected batch retains every returned identity for adapter-owned cleanup.
#[must_use = "callback returns require acknowledgment or retained failure cleanup"]
#[derive(Debug)]
pub(super) enum Completion {
    Settled {
        settlement: SettlementId,
        dispositions: Vec<Disposition>,
    },
    Rejected(CompletionFailure),
}

impl Completion {
    pub fn dispositions(&self) -> &[Disposition] {
        match self {
            Self::Settled { dispositions, .. } => dispositions,
            Self::Rejected(_) => &[],
        }
    }
}

/// Capacity or identity exhaustion cannot discard callback ownership. The adapter must retain
/// this batch, stop requests, and use verified identity-checked cleanup. This book stays blocked.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct CompletionFailure {
    pub reason: CompletionFailureReason,
    pub request: RequestId,
    pub returned: Vec<ReturnedActor>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum CompletionFailureReason {
    Capacity,
    IdentityExhausted,
    Blocked,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct SettlementId(NonZeroU64);

/// A member moves out of source staging once. The loadout adapter consumes this claim through
/// its normal direct pickup route; the other member remains at the source.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct MemberClaim {
    source: SourceKey,
    member: Member,
    pub actor: ActorKey,
    pub weapon: WeaponType,
    pub ammo: GunAmmo,
    pub hand: Hand,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum MemberStatus {
    Available,
    Pending(RequestId),
    Staged(ActorKey),
    Taken,
    Retiring(ActorKey),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Counts {
    pub sources: usize,
    pub outstanding_requests: usize,
    pub cancelled_requests: usize,
    pub awaiting_cleanup: usize,
    pub callback_batches: usize,
    pub pending_disposals: usize,
    pub completion_blocked: bool,
    pub staged: usize,
    pub retiring: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum SourceError {
    Capacity,
    DuplicateSource,
    UnknownSource,
    InvalidSource,
    NoSuchMember,
    WrongOffering,
    MemberUnavailable,
    HandOccupied,
    UnknownRequest,
    AwaitingCleanup,
    IdentityExhausted,
    CleanupOutstanding,
    WrongRetirement,
    ClaimRefused,
    SourceBusy,
    CompletionBlocked,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "loadout source: {self:?}")
    }
}

impl std::error::Error for SourceError {}

/// Only source/member state lives here. Limits bound retained source, request, callback-batch,
/// and disposal-identity metadata. Saturation blocks further adoption without discarding returns.
#[derive(Debug)]
pub(super) struct RequestBook {
    source_limit: usize,
    request_limit: usize,
    sources: Vec<Source>,
    requests: Vec<Request>,
    ids: RequestIds,
    callback_limit: usize,
    callbacks: Vec<CallbackBatch>,
    next_settlement: Option<NonZeroU64>,
    completion_blocked: bool,
}

impl RequestBook {
    pub fn new(source_limit: usize, request_limit: usize) -> Self {
        Self::with_callback_limit(source_limit, request_limit, DEFAULT_CALLBACK_LIMIT)
    }

    /// The same explicit limit bounds callback entries, unacknowledged batches, and retained
    /// disposal identities. Rejected returns remain owned by the caller without classification.
    pub fn with_callback_limit(
        source_limit: usize,
        request_limit: usize,
        callback_limit: usize,
    ) -> Self {
        Self {
            source_limit,
            request_limit,
            sources: Vec::new(),
            requests: Vec::new(),
            ids: RequestIds::default(),
            callback_limit,
            callbacks: Vec::new(),
            next_settlement: Some(NonZeroU64::MIN),
            completion_blocked: false,
        }
    }

    /// Allocate an activation identity from the same sequence as source requests. The request book
    /// must survive world resets while any callback can still arrive.
    pub fn activation_id(&mut self) -> Result<RequestId, SourceError> {
        self.require_completion_ready()?;
        self.ids.issue()
    }

    pub fn add_source(&mut self, key: SourceKey, offering: Offering) -> Result<(), SourceError> {
        if self.sources.iter().any(|source| source.key == key) {
            return Err(SourceError::DuplicateSource);
        }
        if self.sources.len() == self.source_limit {
            return Err(SourceError::Capacity);
        }
        self.sources.push(Source::new(key, offering));
        Ok(())
    }

    /// Supply never examines carried stock or previous draws. Occupancy is checked at commit time
    /// by the eventual table adapter; this value is not itself a successful item-table insertion.
    pub fn draw_safe(
        &self,
        key: SourceKey,
        hand: Hand,
        occupancy: &LoadoutSnapshot,
    ) -> Result<SafeDraw, SourceError> {
        self.require_completion_ready()?;
        let source = self.valid_source(key)?;
        let SourceKind::Grenades(kind) = source.kind else {
            return Err(SourceError::WrongOffering);
        };
        if occupancy.held(hand).is_some() {
            return Err(SourceError::HandOccupied);
        }
        Ok(SafeDraw {
            source: key,
            kind,
            hand,
        })
    }

    /// Begin preparation independently of free hands and holsters. A cancelled request still
    /// occupies its member and a request slot until completion or verified cancellation.
    pub fn begin(&mut self, key: SourceKey, member: Member) -> Result<RequestId, SourceError> {
        self.require_completion_ready()?;
        let source = self.valid_source(key)?;
        source.require_member(member)?;
        if source.members[member.index()] != MemberStatus::Available {
            return Err(SourceError::MemberUnavailable);
        }
        if self.requests.len() == self.request_limit {
            return Err(SourceError::Capacity);
        }
        if self.requests.iter().any(|request| request.source == key) {
            return Err(SourceError::SourceBusy);
        }
        let id = self.ids.issue()?;
        let source = self.source_mut(key)?;
        source.members[member.index()] = MemberStatus::Pending(id);
        self.requests.push(Request {
            id,
            source: key,
            member,
            phase: RequestPhase::Pending,
        });
        Ok(id)
    }

    /// Timeout and user cancellation have identical accounting. Neither proves engine cancellation.
    pub fn cancel(&mut self, id: RequestId) -> Result<(), SourceError> {
        let request = self.request_mut(id)?;
        match request.phase {
            RequestPhase::Pending | RequestPhase::Cancelled => {
                request.phase = RequestPhase::Cancelled
            }
            RequestPhase::Cleanup => return Err(SourceError::AwaitingCleanup),
        }
        Ok(())
    }

    /// Consume one callback. Exactly one correctly typed, unowned actor may stage for a current
    /// pending member. Multi-object batches are rejected whole, with each distinct actor accounted.
    /// `carried` contains current identities adopted by the authoritative loadout, not just addresses.
    pub fn complete(
        &mut self,
        id: RequestId,
        returned: Vec<ReturnedActor>,
        carried: &[ActorKey],
    ) -> Completion {
        if self.completion_blocked {
            return self.reject_completion(CompletionFailureReason::Blocked, id, returned);
        }
        if self.callbacks.len() >= self.callback_limit || returned.len() > self.callback_limit {
            return self.reject_completion(CompletionFailureReason::Capacity, id, returned);
        }
        let Some(next_settlement) = self.next_settlement else {
            return self.reject_completion(
                CompletionFailureReason::IdentityExhausted,
                id,
                returned,
            );
        };
        let request_index = self.requests.iter().position(|request| request.id == id);
        let eligible = request_index.and_then(|index| {
            let request = &self.requests[index];
            let source = self
                .sources
                .iter()
                .find(|source| source.key == request.source)?;
            (request.phase == RequestPhase::Pending
                && source.valid
                && source.members[request.member.index()] == MemberStatus::Pending(id))
            .then_some((source.key, request.member, source.kind))
        });
        let accept = returned.first().and_then(|first| {
            let (source, member, SourceKind::Guns { weapon, .. }) = eligible? else {
                return None;
            };
            (returned.len() == 1
                && first.weapon == weapon
                && !self.owned(first.actor, carried)
                && !self.cleanup_pending(first.actor)
                && !self.identity_conflict(first.actor, carried))
            .then_some((source, member, first.actor))
        });
        let mut seen = Vec::new();
        let mut dispositions = Vec::with_capacity(returned.len());
        let mut disposal = Vec::new();
        for entry in &returned {
            let disposition = if seen.contains(&entry.actor) {
                Disposition::Repeated(entry.actor)
            } else if accept.is_some_and(|(_, _, actor)| actor == entry.actor) {
                Disposition::Staged(entry.actor)
            } else if self.owned(entry.actor, carried) {
                Disposition::AlreadyOwned(entry.actor)
            } else if self.cleanup_pending(entry.actor) {
                Disposition::CleanupPending(entry.actor)
            } else if self.identity_conflict(entry.actor, carried)
                || seen
                    .iter()
                    .any(|known: &ActorKey| known.address == entry.actor.address)
            {
                Disposition::IdentityConflict(entry.actor)
            } else {
                disposal.push(entry.actor);
                Disposition::Dispose(entry.actor)
            };
            seen.push(entry.actor);
            dispositions.push(disposition);
        }
        let retained = self
            .callbacks
            .iter()
            .map(|batch| batch.disposal.len())
            .sum::<usize>();
        if disposal.len() > self.callback_limit.saturating_sub(retained) {
            return self.reject_completion(CompletionFailureReason::Capacity, id, returned);
        }
        if let Some((source, member, actor)) = accept {
            self.sources
                .iter_mut()
                .find(|entry| entry.key == source)
                .expect("source exists")
                .members[member.index()] = MemberStatus::Staged(actor);
        }
        if let Some(index) = request_index {
            self.requests[index].phase = RequestPhase::Cleanup;
        }
        let settlement = SettlementId(next_settlement);
        self.next_settlement = next_settlement.checked_add(1);
        self.callbacks.push(CallbackBatch {
            settlement,
            request: request_index.map(|_| id),
            disposal,
        });
        Completion::Settled {
            settlement,
            dispositions,
        }
    }

    /// Record completed disposal/queue handoff for this batch only. Every admitted callback,
    /// including a late or unknown callback, has its own receipt. A request releases only after
    /// all its receipts settle. A rejected or unrelated receipt is returned unchanged on error.
    /// After saturation the book stays blocked, even when earlier admitted batches settle.
    pub fn acknowledge(&mut self, completion: Completion) -> Result<(), Completion> {
        let Completion::Settled { settlement, .. } = &completion else {
            return Err(completion);
        };
        let Some(index) = self
            .callbacks
            .iter()
            .position(|batch| batch.settlement == *settlement)
        else {
            return Err(completion);
        };
        let batch = self.callbacks.remove(index);
        let Some(id) = batch.request else {
            return Ok(());
        };
        if self.completion_blocked || self.callbacks.iter().any(|batch| batch.request == Some(id)) {
            return Ok(());
        }
        let index = self
            .requests
            .iter()
            .position(|request| request.id == id)
            .expect("an unacknowledged request cannot be removed");
        let request = self.requests.remove(index);
        let source = self
            .source_mut(request.source)
            .expect("an outstanding source cannot be forgotten");
        if source.members[request.member.index()] == MemberStatus::Pending(id) {
            source.members[request.member.index()] = MemberStatus::Available;
        }
        Ok(())
    }

    /// Call only after a verified engine boundary proves that no callback can still arrive.
    /// Timeout alone never authorizes this operation.
    pub fn confirm_cancelled(&mut self, id: RequestId) -> Result<(), SourceError> {
        self.require_completion_ready()?;
        let index = self
            .requests
            .iter()
            .position(|request| request.id == id)
            .ok_or(SourceError::UnknownRequest)?;
        if self.requests[index].phase != RequestPhase::Cancelled {
            return Err(SourceError::CleanupOutstanding);
        }
        let request = self.requests.remove(index);
        let source = self.source_mut(request.source)?;
        source.members[request.member.index()] = MemberStatus::Available;
        Ok(())
    }

    /// Commit through the ordinary loadout take. A refusal must return the same claim unchanged;
    /// that restores source staging and its ammunition allocation without making a second copy.
    /// The closure runs synchronously, so invalidation cannot interleave with this handoff.
    pub fn claim<R>(
        &mut self,
        key: SourceKey,
        member: Member,
        hand: Hand,
        occupancy: &LoadoutSnapshot,
        accept: impl FnOnce(MemberClaim) -> Result<R, MemberClaim>,
    ) -> Result<R, SourceError> {
        self.require_completion_ready()?;
        let source = self.valid_source(key)?;
        source.require_member(member)?;
        if occupancy.held(hand).is_some() {
            return Err(SourceError::HandOccupied);
        }
        let MemberStatus::Staged(actor) = source.members[member.index()] else {
            return Err(SourceError::MemberUnavailable);
        };
        let SourceKind::Guns { weapon, .. } = source.kind else {
            return Err(SourceError::WrongOffering);
        };
        let source = self.source_mut(key)?;
        let ammo = source.ammo[member.index()]
            .take()
            .expect("an unclaimed member owns its allocation");
        source.members[member.index()] = MemberStatus::Taken;
        match accept(MemberClaim {
            source: key,
            member,
            actor,
            weapon,
            ammo,
            hand,
        }) {
            Ok(value) => Ok(value),
            Err(claim) => {
                assert_eq!(
                    (claim.source, claim.member, claim.actor),
                    (key, member, actor)
                );
                source.ammo[member.index()] = Some(claim.ammo);
                source.members[member.index()] = MemberStatus::Staged(actor);
                Err(SourceError::ClaimRefused)
            }
        }
    }

    pub fn member(&self, key: SourceKey, member: Member) -> Result<MemberStatus, SourceError> {
        let source = self.source(key)?;
        source.require_member(member)?;
        Ok(source.members[member.index()])
    }

    /// Invalidate a destroyed source incarnation. This is not a persistence policy for temporary
    /// streaming. Staged actors remain explicit retirement obligations until acknowledged.
    pub fn invalidate(&mut self, key: SourceKey) -> Result<(), SourceError> {
        let source = self.source_mut(key)?;
        source.valid = false;
        for member in &mut source.members {
            if let MemberStatus::Staged(actor) = *member {
                *member = MemberStatus::Retiring(actor);
            }
        }
        for request in &mut self.requests {
            if request.source == key && request.phase == RequestPhase::Pending {
                request.phase = RequestPhase::Cancelled;
            }
        }
        Ok(())
    }

    pub fn confirm_retired(
        &mut self,
        key: SourceKey,
        member: Member,
        actor: ActorKey,
    ) -> Result<(), SourceError> {
        let source = self.source_mut(key)?;
        source.require_member(member)?;
        if source.members[member.index()] != MemberStatus::Retiring(actor) {
            return Err(SourceError::WrongRetirement);
        }
        source.members[member.index()] = MemberStatus::Available;
        Ok(())
    }

    /// Forget only an invalidated source with no remaining callback or retirement obligation.
    pub fn forget(&mut self, key: SourceKey) -> Result<(), SourceError> {
        let source = self.source(key)?;
        if source.valid
            || self.requests.iter().any(|request| request.source == key)
            || source
                .members
                .iter()
                .any(|member| matches!(member, MemberStatus::Staged(_) | MemberStatus::Retiring(_)))
        {
            return Err(SourceError::CleanupOutstanding);
        }
        self.sources.retain(|source| source.key != key);
        Ok(())
    }

    pub fn counts(&self) -> Counts {
        let members = || self.sources.iter().flat_map(|source| source.members.iter());
        Counts {
            sources: self.sources.len(),
            outstanding_requests: self.requests.len(),
            cancelled_requests: self
                .requests
                .iter()
                .filter(|request| request.phase == RequestPhase::Cancelled)
                .count(),
            awaiting_cleanup: self
                .requests
                .iter()
                .filter(|request| request.phase == RequestPhase::Cleanup)
                .count(),
            callback_batches: self.callbacks.len(),
            pending_disposals: self
                .callbacks
                .iter()
                .map(|batch| batch.disposal.len())
                .sum(),
            completion_blocked: self.completion_blocked,
            staged: members()
                .filter(|member| matches!(member, MemberStatus::Staged(_)))
                .count(),
            retiring: members()
                .filter(|member| matches!(member, MemberStatus::Retiring(_)))
                .count(),
        }
    }

    fn source(&self, key: SourceKey) -> Result<&Source, SourceError> {
        self.sources
            .iter()
            .find(|source| source.key == key)
            .ok_or(SourceError::UnknownSource)
    }

    fn valid_source(&self, key: SourceKey) -> Result<&Source, SourceError> {
        let source = self.source(key)?;
        if !source.valid {
            return Err(SourceError::InvalidSource);
        }
        Ok(source)
    }

    fn source_mut(&mut self, key: SourceKey) -> Result<&mut Source, SourceError> {
        self.sources
            .iter_mut()
            .find(|source| source.key == key)
            .ok_or(SourceError::UnknownSource)
    }

    fn request_mut(&mut self, id: RequestId) -> Result<&mut Request, SourceError> {
        self.requests
            .iter_mut()
            .find(|request| request.id == id)
            .ok_or(SourceError::UnknownRequest)
    }

    fn require_completion_ready(&self) -> Result<(), SourceError> {
        if self.completion_blocked {
            Err(SourceError::CompletionBlocked)
        } else {
            Ok(())
        }
    }

    fn reject_completion(
        &mut self,
        reason: CompletionFailureReason,
        request: RequestId,
        returned: Vec<ReturnedActor>,
    ) -> Completion {
        self.completion_blocked = true;
        Completion::Rejected(CompletionFailure {
            reason,
            request,
            returned,
        })
    }

    fn cleanup_pending(&self, actor: ActorKey) -> bool {
        self.callbacks
            .iter()
            .any(|batch| batch.disposal.contains(&actor))
    }

    fn identity_conflict(&self, actor: ActorKey, carried: &[ActorKey]) -> bool {
        let differs = |known: &ActorKey| known.address == actor.address && *known != actor;
        carried.iter().any(differs)
            || self
                .callbacks
                .iter()
                .flat_map(|batch| batch.disposal.iter())
                .any(differs)
            || self
                .sources
                .iter()
                .flat_map(|source| source.members.iter())
                .any(|member| match member {
                    MemberStatus::Staged(known) | MemberStatus::Retiring(known) => differs(known),
                    _ => false,
                })
    }

    fn owned(&self, actor: ActorKey, carried: &[ActorKey]) -> bool {
        carried.contains(&actor) || self.sources.iter().any(|source| source.members.iter().any(|member| {
            matches!(member, MemberStatus::Staged(known) | MemberStatus::Retiring(known) if *known == actor)
        }))
    }
}

#[derive(Debug)]
struct Source {
    key: SourceKey,
    valid: bool,
    kind: SourceKind,
    members: [MemberStatus; 2],
    ammo: [Option<GunAmmo>; 2],
}

impl Source {
    fn new(key: SourceKey, offering: Offering) -> Self {
        let (kind, ammo) = match offering {
            Offering::UnlimitedGrenades { kind } => (SourceKind::Grenades(kind), [None, None]),
            Offering::Single { weapon, ammo } => {
                (SourceKind::Guns { weapon, count: 1 }, [Some(ammo), None])
            }
            Offering::Pair {
                weapon,
                ammo: [first, second],
            } => (
                SourceKind::Guns { weapon, count: 2 },
                [Some(first), Some(second)],
            ),
        };
        Self {
            key,
            valid: true,
            kind,
            members: [MemberStatus::Available; 2],
            ammo,
        }
    }

    fn require_member(&self, member: Member) -> Result<(), SourceError> {
        match self.kind {
            SourceKind::Guns { count, .. } if member.index() < count => Ok(()),
            _ => Err(SourceError::NoSuchMember),
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum SourceKind {
    Grenades(u32),
    Guns { weapon: WeaponType, count: usize },
}

impl Member {
    fn index(self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }
}

const DEFAULT_CALLBACK_LIMIT: usize = 64;

#[derive(Debug)]
struct CallbackBatch {
    settlement: SettlementId,
    request: Option<RequestId>,
    disposal: Vec<ActorKey>,
}

#[derive(Debug)]
struct Request {
    id: RequestId,
    source: SourceKey,
    member: Member,
    phase: RequestPhase,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum RequestPhase {
    Pending,
    Cancelled,
    Cleanup,
}

#[cfg(test)]
#[path = "lifecycle_source_tests.rs"]
mod tests;
