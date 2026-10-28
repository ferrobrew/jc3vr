//! Unload containment for the inventory's restore and second-pistol spawn callbacks.
//!
//! Native request retirement is not established here. Exposure therefore remains latched for the
//! process lifetime, even after a callback returns, a timeout, or a world reset. Preparation only
//! excludes new submissions when no callback has been exposed; it performs no native work.

use std::{
    fmt,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;

use crate::lifecycle::{
    DeferReason, Preparation, PrepareOutcome, PreparedParticipant, ShutdownParticipant,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum RequestKind {
    Restore,
    Twin,
}

/// Refusal occurs before the native submission closure runs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct AdmissionClosed;

impl fmt::Display for AdmissionClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("inventory spawn: submission admission is closed")
    }
}

impl std::error::Error for AdmissionClosed {}

/// A failed final check requires the existing module-pin path after hook removal.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct UnloadBlocked {
    first_exposure: Option<RequestKind>,
    callback_entered: bool,
    submissions_in_flight: usize,
    active_callbacks: usize,
    admission: Admission,
}

impl fmt::Display for UnloadBlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "inventory spawn unload: native callback retirement is unproven; first exposure {:?}, callback entered {}, submissions in flight {}, active callbacks {}, admission {:?}",
            self.first_exposure,
            self.callback_entered,
            self.submissions_in_flight,
            self.active_callbacks,
            self.admission,
        )
    }
}

impl std::error::Error for UnloadBlocked {}

pub(crate) fn participant() -> Arc<dyn ShutdownParticipant> {
    SAFETY.clone()
}

/// Open only after shutdown participation and the inventory hooks are installed.
pub(crate) fn activate() {
    SAFETY.activate();
}

pub(crate) fn submit<T>(
    kind: RequestKind,
    native: impl FnOnce() -> T,
) -> Result<T, AdmissionClosed> {
    SAFETY.submit(kind, native)
}

pub(crate) fn callback_entry() -> CallbackEntry {
    SAFETY.callback_entry()
}

pub(crate) fn post_unpatch_check() -> Result<(), UnloadBlocked> {
    SAFETY.post_unpatch_check()
}

pub(crate) struct CallbackEntry {
    owner: Arc<Owner>,
}

impl Drop for CallbackEntry {
    fn drop(&mut self) {
        self.owner.active_callbacks.fetch_sub(1, Ordering::Release);
    }
}

static SAFETY: LazyLock<Arc<SpawnSafety>> = LazyLock::new(|| Arc::new(SpawnSafety::new()));

pub(crate) struct SpawnSafety {
    owner: Arc<Owner>,
}

#[cfg(test)]
impl SpawnSafety {
    pub(crate) fn for_test() -> Self {
        Self::new()
    }

    pub(crate) fn activate_for_test(&self) {
        self.activate();
    }

    pub(crate) fn submit_for_test<T>(
        &self,
        kind: RequestKind,
        native: impl FnOnce() -> T,
    ) -> Result<T, AdmissionClosed> {
        self.submit(kind, native)
    }

    pub(crate) fn post_unpatch_for_test(&self) -> Result<(), UnloadBlocked> {
        self.post_unpatch_check()
    }
}

impl SpawnSafety {
    fn new() -> Self {
        Self {
            owner: Arc::new(Owner {
                state: Mutex::new(State {
                    admission: Admission::Uninstalled,
                    first_exposure: None,
                    submissions_in_flight: 0,
                }),
                active_callbacks: AtomicUsize::new(0),
                callback_entered: AtomicBool::new(false),
            }),
        }
    }

    fn activate(&self) {
        let mut state = self.owner.state.lock();
        if state.admission == Admission::Uninstalled {
            state.admission = Admission::Open;
        }
    }

    fn submit<T>(
        &self,
        kind: RequestKind,
        native: impl FnOnce() -> T,
    ) -> Result<T, AdmissionClosed> {
        let submission = {
            let mut state = self.owner.state.lock();
            if state.admission != Admission::Open {
                return Err(AdmissionClosed);
            }
            // The engine can retain the callback even when it never reports a result.
            state.first_exposure.get_or_insert(kind);
            state.submissions_in_flight += 1;
            Submission { owner: &self.owner }
        };
        let result = native();
        drop(submission);
        Ok(result)
    }

    fn callback_entry(&self) -> CallbackEntry {
        self.owner.active_callbacks.fetch_add(1, Ordering::AcqRel);
        self.owner.callback_entered.store(true, Ordering::Release);
        CallbackEntry {
            owner: Arc::clone(&self.owner),
        }
    }

    fn post_unpatch_check(&self) -> Result<(), UnloadBlocked> {
        let state = self.owner.state.lock();
        let status = UnloadBlocked {
            first_exposure: state.first_exposure,
            callback_entered: self.owner.callback_entered.load(Ordering::Acquire),
            submissions_in_flight: state.submissions_in_flight,
            active_callbacks: self.owner.active_callbacks.load(Ordering::Acquire),
            admission: state.admission,
        };
        if status.first_exposure.is_none()
            && !status.callback_entered
            && status.submissions_in_flight == 0
            && status.active_callbacks == 0
            && status.admission == Admission::Committed
        {
            Ok(())
        } else {
            Err(status)
        }
    }
}

impl ShutdownParticipant for SpawnSafety {
    fn prepare(&self, preparation: Preparation) -> PrepareOutcome<Box<dyn PreparedParticipant>> {
        let mut state = self.owner.state.lock();
        if state.first_exposure.is_some() {
            return PrepareOutcome::Deferred(DeferReason::SpawnCallbacksUnproven);
        }
        if state.submissions_in_flight != 0
            || self.owner.active_callbacks.load(Ordering::Acquire) != 0
        {
            return PrepareOutcome::Deferred(DeferReason::SpawnCallbackActive);
        }
        if self.owner.callback_entered.load(Ordering::Acquire) {
            return PrepareOutcome::Deferred(DeferReason::SpawnCallbacksUnproven);
        }
        if matches!(
            state.admission,
            Admission::Preparing(_) | Admission::Committed
        ) {
            return PrepareOutcome::Deferred(DeferReason::ParticipantBusy(
                crate::lifecycle::ParticipantId::InventorySpawn,
            ));
        }
        let previous = state.admission;
        state.admission = Admission::Preparing(preparation);
        PrepareOutcome::Ready(Box::new(PreparedSpawn {
            owner: Arc::clone(&self.owner),
            preparation,
            previous,
            committed: false,
        }))
    }
}

struct Owner {
    state: Mutex<State>,
    active_callbacks: AtomicUsize,
    callback_entered: AtomicBool,
}

struct State {
    admission: Admission,
    first_exposure: Option<RequestKind>,
    submissions_in_flight: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Admission {
    Uninstalled,
    Open,
    Preparing(Preparation),
    Committed,
}

struct Submission<'a> {
    owner: &'a Owner,
}

impl Drop for Submission<'_> {
    fn drop(&mut self) {
        self.owner.state.lock().submissions_in_flight -= 1;
    }
}

struct PreparedSpawn {
    owner: Arc<Owner>,
    preparation: Preparation,
    previous: Admission,
    committed: bool,
}

impl PreparedParticipant for PreparedSpawn {
    fn validate(&self, preparation: Preparation) -> bool {
        let state = self.owner.state.lock();
        self.preparation == preparation
            && state.admission == Admission::Preparing(preparation)
            && state.first_exposure.is_none()
            && !self.owner.callback_entered.load(Ordering::Acquire)
            && state.submissions_in_flight == 0
            && self.owner.active_callbacks.load(Ordering::Acquire) == 0
    }

    fn commit(&mut self) {
        self.owner.state.lock().admission = Admission::Committed;
        self.committed = true;
    }
}

impl Drop for PreparedSpawn {
    fn drop(&mut self) {
        if !self.committed {
            let mut state = self.owner.state.lock();
            if state.admission == Admission::Preparing(self.preparation) {
                state.admission = self.previous;
            }
        }
    }
}

#[cfg(test)]
mod tests;
