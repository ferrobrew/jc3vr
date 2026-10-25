//! Bounded execution of the existing G05 request contract with a controlled engine.
//!
//! Admission exists only in tests. No actor is staged: callbacks cancel the G05 request before
//! classification because source qualification, ammunition, and inert staging remain unavailable.
//! Group identities remain recorded until this executor is retired; saturation is terminal rather
//! than evicting the identity needed to recognize a late duplicate.

use std::{fmt, num::NonZeroU64, sync::Arc};

use crate::controllers::loadout::{
    lifecycle_source::{Completion, Counts, Member, RequestBook, SourceError, SourceKey},
    spawn_transport::ownership::{
        Callback, CleanupProgress, Engine, Epoch, GroupId, OwnedGroup, ReleaseReason, Ticket,
    },
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Limits {
    pub requests: usize,
    pub groups: usize,
    pub objects_per_group: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Error {
    Closed,
    Capacity,
    DuplicateGroup,
    EpochExhausted,
    BoundaryExhausted,
    BoundaryOutstanding,
    WrongBoundary,
    StaleEpoch,
    Source(SourceError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "loadout spawn transport: {self:?}")
    }
}

impl std::error::Error for Error {}

/// No public constructor can claim a native quiescent phase. Only the controlled test engine
/// mints one. A future native implementation needs independently reviewed signal/wait proof.
#[derive(Debug)]
pub(super) struct Quiescent {
    epoch: Epoch,
}

impl Quiescent {
    pub fn controlled(epoch: Epoch) -> Self {
        Self { epoch }
    }
}

/// Non-clone token tying acknowledgments to the exact service invocation after submission.
#[derive(Debug)]
pub(super) struct Boundary {
    sequence: NonZeroU64,
    epoch: Epoch,
    owner: Arc<()>,
}

#[derive(Debug)]
pub(super) struct ServiceReturned {
    sequence: NonZeroU64,
    epoch: Epoch,
    owner: Arc<()>,
}

#[must_use = "the callback refusal still owns every returned reference"]
#[derive(Debug)]
pub(super) struct HandoffFailure<R> {
    pub reason: Error,
    pub callback: Callback<R>,
}

/// A capacity refusal is caller-owned. It cannot be placed in another hidden queue. Cleanup may
/// be submitted only at a quiescent boundary, and its retained references survive until return.
#[must_use = "the refused callback owns a whole-group cleanup obligation"]
#[derive(Debug)]
pub(super) struct OverflowCleanup<R> {
    callback: Callback<R>,
    boundary: NonZeroU64,
    epoch: Epoch,
    owner: Arc<()>,
}

impl<R> HandoffFailure<R> {
    pub fn queue_cleanup(
        self,
        boundary: &Boundary,
        engine: &mut impl Engine<Strong = R>,
    ) -> Result<OverflowCleanup<R>, Self> {
        if self.reason != Error::Capacity {
            return Err(self);
        }
        let count = self.callback.group.objects.len();
        if count != 0
            && engine.cleanup_group(&self.callback.group)
                != (CleanupProgress::Queued { objects: count })
        {
            return Err(self);
        }
        Ok(OverflowCleanup {
            callback: self.callback,
            boundary: boundary.sequence,
            epoch: boundary.epoch,
            owner: Arc::clone(&boundary.owner),
        })
    }
}

impl<R> OverflowCleanup<R> {
    pub fn after_pre_update(
        self,
        returned: &ServiceReturned,
        engine: &mut impl Engine<Strong = R>,
    ) -> Result<(), Self> {
        if self.boundary != returned.sequence
            || self.epoch != returned.epoch
            || !Arc::ptr_eq(&self.owner, &returned.owner)
        {
            return Err(self);
        }
        engine.release_group(self.callback.group, ReleaseReason::CleanupBoundary);
        Ok(())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Status {
    pub closed: bool,
    pub faulted: bool,
    pub epoch: Epoch,
    pub requests: usize,
    pub retained_groups: usize,
    pub group_records: usize,
    pub unresolved_groups: usize,
    pub request_book: Counts,
}

/// This owns the G05 book rather than copying its source/member state into another registry.
#[derive(Debug)]
pub(super) struct Transport<R> {
    book: RequestBook,
    limits: Limits,
    epoch: Epoch,
    closed: bool,
    faulted: bool,
    requests: Vec<PendingRequest>,
    groups: Vec<Group<R>>,
    next_boundary: Option<NonZeroU64>,
    active_boundary: Option<NonZeroU64>,
    boundary_owner: Arc<()>,
}

impl<R> Transport<R> {
    pub fn controlled(book: RequestBook, limits: Limits) -> Self {
        Self {
            book,
            limits,
            epoch: Epoch(1),
            closed: false,
            faulted: false,
            requests: Vec::new(),
            groups: Vec::new(),
            next_boundary: Some(NonZeroU64::MIN),
            active_boundary: None,
            boundary_owner: Arc::new(()),
        }
    }

    pub fn begin(&mut self, source: SourceKey, member: Member) -> Result<Ticket, Error> {
        if self.closed || self.faulted {
            return Err(Error::Closed);
        }
        if self.requests.len() >= self.limits.requests {
            return Err(Error::Capacity);
        }
        let request = self.book.begin(source, member).map_err(Error::Source)?;
        let ticket = Ticket {
            request,
            epoch: self.epoch,
        };
        self.requests.push(PendingRequest {
            ticket,
            phase: RequestPhase::Queued,
            cancel: false,
        });
        Ok(ticket)
    }

    /// Cancellation before native submission needs no native acknowledgment. Cancellation of a
    /// submitted request occupies its slot until a later matching service boundary returns.
    pub fn cancel(&mut self, ticket: Ticket) -> Result<(), Error> {
        self.book.cancel(ticket.request).map_err(Error::Source)?;
        let index = self
            .requests
            .iter()
            .position(|request| request.ticket == ticket)
            .ok_or(Error::Source(SourceError::UnknownRequest))?;
        if self.requests[index].phase == RequestPhase::Queued {
            self.book
                .confirm_cancelled(ticket.request)
                .map_err(Error::Source)?;
            self.requests.remove(index);
        } else {
            self.requests[index].cancel = true;
        }
        Ok(())
    }

    /// Close never erases requests, callback groups, or group identity records.
    pub fn close(&mut self) {
        self.closed = true;
        for request in &mut self.requests {
            request.cancel = true;
            let _ = self.book.cancel(request.ticket.request);
        }
    }

    /// Call before native reset can service callbacks. Old callbacks remain cleanup-only.
    pub fn reset(&mut self) -> Result<(), Error> {
        self.close();
        let Some(epoch) = self.epoch.0.checked_add(1) else {
            self.faulted = true;
            return Err(Error::EpochExhausted);
        };
        self.epoch = Epoch(epoch);
        Ok(())
    }

    /// Transfer a retained callback without executing engine work under the callback's lock.
    /// Overflow returns the complete owned group, closes admission, and creates no unbounded
    /// fallback collection. The caller must keep the refusal in an owning retirement slot.
    pub fn callback(&mut self, callback: Callback<R>) -> Result<(), HandoffFailure<R>> {
        if self
            .groups
            .iter()
            .any(|group| group.id == callback.group.id && group.ticket != callback.group.ticket)
        {
            self.fail();
            return Err(HandoffFailure {
                reason: Error::StaleEpoch,
                callback,
            });
        }
        let duplicate = self
            .groups
            .iter()
            .any(|group| group.id == callback.group.id);
        if self.groups.len() >= self.limits.groups {
            self.fail();
            return Err(HandoffFailure {
                reason: if duplicate {
                    Error::DuplicateGroup
                } else {
                    Error::Capacity
                },
                callback,
            });
        }
        let oversized = callback.group.objects.len() > self.limits.objects_per_group;
        let ticket = callback.group.ticket;
        if let Some(request) = self
            .requests
            .iter_mut()
            .find(|request| request.ticket == ticket)
        {
            request.phase = RequestPhase::Delivered;
            let _ = self.book.cancel(ticket.request);
        }
        let _ = self.book.cancel(ticket.request);
        let receipt = callback
            .classified
            .map(|returned| self.book.complete(ticket.request, returned, &[]));
        if oversized
            || receipt
                .as_ref()
                .is_some_and(|receipt| matches!(receipt, Completion::Rejected(_)))
        {
            self.fail();
        }
        self.groups.push(Group {
            id: callback.group.id,
            ticket,
            owned: Some(callback.group),
            receipt,
            phase: if duplicate {
                GroupPhase::Duplicate
            } else {
                GroupPhase::Ready
            },
        });
        Ok(())
    }

    /// Queue commands before the original service call, but do not acknowledge them here.
    pub fn before_pre_update(
        &mut self,
        quiescent: Quiescent,
        engine: &mut impl Engine<Strong = R>,
    ) -> Result<Boundary, Error> {
        if quiescent.epoch != self.epoch {
            return Err(Error::StaleEpoch);
        }
        if self.active_boundary.is_some() {
            return Err(Error::BoundaryOutstanding);
        }
        let Some(sequence) = self.next_boundary else {
            self.fail();
            return Err(Error::BoundaryExhausted);
        };
        self.next_boundary = sequence.checked_add(1);
        self.active_boundary = Some(sequence);
        for request in &mut self.requests {
            match request.phase {
                RequestPhase::Queued if request.cancel => {
                    let _ = self.book.confirm_cancelled(request.ticket.request);
                    request.phase = RequestPhase::LocallyCancelled;
                }
                RequestPhase::Queued => {
                    engine.spawn(request.ticket);
                    request.phase = RequestPhase::Submitted;
                }
                RequestPhase::Submitted if request.cancel => {
                    engine.cancel(request.ticket);
                    request.phase = RequestPhase::CancelQueued(sequence);
                }
                _ => {}
            }
        }
        for group in &mut self.groups {
            if group.phase != GroupPhase::Ready {
                continue;
            }
            let owned = group.owned.as_ref().expect("ready group owns every object");
            group.phase = if owned.objects.is_empty() {
                GroupPhase::CleanupQueued(sequence)
            } else {
                match engine.cleanup_group(owned) {
                    CleanupProgress::Queued { objects } if objects == owned.objects.len() => {
                        GroupPhase::CleanupQueued(sequence)
                    }
                    CleanupProgress::Queued { .. } | CleanupProgress::Indeterminate => {
                        self.closed = true;
                        self.faulted = true;
                        GroupPhase::Indeterminate
                    }
                }
            };
        }
        Ok(Boundary {
            sequence,
            epoch: self.epoch,
            owner: Arc::clone(&self.boundary_owner),
        })
    }

    /// Called only after the corresponding original service returns. A callback arriving inside
    /// that service is not cleaned or acknowledged by an earlier command submission.
    pub fn after_pre_update(
        &mut self,
        boundary: Boundary,
        engine: &mut impl Engine<Strong = R>,
    ) -> Result<ServiceReturned, Error> {
        if self.active_boundary != Some(boundary.sequence)
            || !Arc::ptr_eq(&self.boundary_owner, &boundary.owner)
        {
            self.fail();
            return Err(Error::WrongBoundary);
        }
        self.active_boundary = None;
        for group in &mut self.groups {
            if group.phase == GroupPhase::CleanupQueued(boundary.sequence) {
                let owned = group
                    .owned
                    .take()
                    .expect("queued group retains all objects");
                engine.release_group(owned, ReleaseReason::CleanupBoundary);
                group.phase = GroupPhase::Retired;
            }
        }
        for index in 0..self.groups.len() {
            if self.groups[index].phase != GroupPhase::Duplicate {
                continue;
            }
            let id = self.groups[index].id;
            let retired = self
                .groups
                .iter()
                .any(|group| group.id == id && group.phase == GroupPhase::Retired);
            if retired {
                let group = &mut self.groups[index];
                let owned = group
                    .owned
                    .take()
                    .expect("duplicate retains its references");
                engine.release_group(owned, ReleaseReason::DuplicateReference);
                group.phase = GroupPhase::Retired;
            }
        }
        for group in &mut self.groups {
            if group.phase == GroupPhase::Retired
                && let Some(receipt) = group.receipt.take()
                && let Err(receipt) = self.book.acknowledge(receipt)
            {
                group.receipt = Some(receipt);
            }
        }
        for request in &mut self.requests {
            if request.phase == RequestPhase::CancelQueued(boundary.sequence)
                && self.book.request_outstanding(request.ticket.request)
            {
                match self.book.confirm_cancelled(request.ticket.request) {
                    Ok(()) => request.phase = RequestPhase::LocallyCancelled,
                    Err(SourceError::CleanupOutstanding | SourceError::CompletionBlocked) => {}
                    Err(error) => return Err(Error::Source(error)),
                }
            }
            if request.phase == RequestPhase::Delivered
                && !self.groups.iter().any(|group| {
                    group.ticket == request.ticket
                        && (group.owned.is_some() || group.receipt.is_some())
                })
                && self.book.request_outstanding(request.ticket.request)
            {
                self.book
                    .confirm_cancelled(request.ticket.request)
                    .map_err(Error::Source)?;
            }
        }
        self.requests.retain(|request| {
            self.book.request_outstanding(request.ticket.request)
                || self.groups.iter().any(|group| {
                    group.ticket == request.ticket
                        && (group.owned.is_some() || group.receipt.is_some())
                })
        });
        Ok(ServiceReturned {
            sequence: boundary.sequence,
            epoch: boundary.epoch,
            owner: boundary.owner,
        })
    }

    pub fn status(&self) -> Status {
        Status {
            closed: self.closed,
            faulted: self.faulted,
            epoch: self.epoch,
            requests: self.requests.len(),
            retained_groups: self
                .groups
                .iter()
                .filter(|group| group.owned.is_some())
                .count(),
            group_records: self.groups.len(),
            unresolved_groups: self
                .groups
                .iter()
                .filter(|group| group.phase == GroupPhase::Indeterminate || group.receipt.is_some())
                .count(),
            request_book: self.book.counts(),
        }
    }

    /// Draining is not callback retirement: native activation would also need exclusion of new
    /// callback entries before releasing code or replacing the session's non-reusing ID allocator.
    pub fn drained(&self) -> bool {
        self.closed
            && !self.faulted
            && self.active_boundary.is_none()
            && self.requests.is_empty()
            && self
                .groups
                .iter()
                .all(|group| group.owned.is_none() && group.receipt.is_none())
    }

    fn fail(&mut self) {
        self.faulted = true;
        self.close();
    }
}

#[derive(Debug)]
struct PendingRequest {
    ticket: Ticket,
    phase: RequestPhase,
    cancel: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum RequestPhase {
    Queued,
    Submitted,
    CancelQueued(NonZeroU64),
    Delivered,
    LocallyCancelled,
}

#[derive(Debug)]
struct Group<R> {
    id: GroupId,
    ticket: Ticket,
    owned: Option<OwnedGroup<R>>,
    receipt: Option<Completion>,
    phase: GroupPhase,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum GroupPhase {
    Ready,
    Duplicate,
    CleanupQueued(NonZeroU64),
    Indeterminate,
    Retired,
}
