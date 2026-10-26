//! Retryable shutdown preparation and one-shot subsystem cleanup.
//!
//! Preparation runs before the irreversible boundary on the game thread. A deferred attempt keeps
//! ordinary cleanups, hooks, and subsystem owners live. Prepared participants hold their exclusion
//! guards through final validation and commit; dropping an uncommitted guard aborts preparation.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use parking_lot::Mutex;

/// The participants with reversible work that must precede all ordinary cleanup.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ParticipantId {
    MagazineExecutor,
    #[allow(dead_code)] // Reserved until the reversible collider adapter is installed.
    HumanCollider,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum DeferReason {
    MagazineBusy,
    ParticipantBusy(ParticipantId),
    StalePreparation,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct PrepareError {
    pub(crate) participant: ParticipantId,
    pub(crate) operation: &'static str,
}

impl fmt::Display for PrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "shutdown preparation: {:?}: {}",
            self.participant, self.operation
        )
    }
}

impl std::error::Error for PrepareError {}

/// Authority for one preparation attempt. A retry invalidates every earlier attempt.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Preparation {
    request: u64,
    attempt: u64,
    revision: u64,
}

pub(crate) enum PrepareOutcome<T> {
    Ready(T),
    Deferred(DeferReason),
    #[allow(dead_code)] // The collider adapter can fail after transactional rollback.
    Failed(PrepareError),
}

/// A participant must leave full gameplay live on Deferred or Failed.
///
/// Ready retains the synchronization needed for `validate` and `commit`. No engine query runs
/// while the lifecycle mutex is held. A participant cannot register another participant here.
pub(crate) trait ShutdownParticipant: Send + Sync {
    fn prepare(&self, preparation: Preparation) -> PrepareOutcome<Box<dyn PreparedParticipant>>;
}

/// Dropping this guard before `commit` must abort all temporary preparation, including rollback.
///
/// `validate` is the last fallible operation. `commit` must only finalize already-restored state.
/// It cannot query the engine, re-enter lifecycle, fail, or release exclusion before guard drop.
/// A participant that cannot meet this contract must return Deferred instead of Ready.
/// This is not a physics-write capability.
pub(crate) trait PreparedParticipant {
    fn validate(&self, preparation: Preparation) -> bool;
    fn commit(&mut self);
}

/// Register before enabling the participant's engine policy or accepting its work.
///
/// The collider participant must restore every affected body transactionally during preparation,
/// retain the proven reader barrier, and roll back on guard drop until the whole attempt commits.
pub(crate) fn register_participant(
    id: ParticipantId,
    participant: Arc<dyn ShutdownParticipant>,
) -> Result<(), RegistrationError> {
    SHUTDOWN.lock().register(id, participant)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum RegistrationError {
    Duplicate(ParticipantId),
    ShutdownStarted,
}

impl fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate(id) => write!(f, "shutdown registration: duplicate participant {id:?}"),
            Self::ShutdownStarted => write!(f, "shutdown registration: shutdown has started"),
        }
    }
}

impl std::error::Error for RegistrationError {}

/// Coalesce requests without changing gameplay or consuming cleanup registrations.
pub(crate) fn request_shutdown() {
    if SHUTDOWN.lock().request() {
        tracing::info!("shutdown requested; preparing before teardown");
    }
}

/// Try at most once per retry interval, from the pre-update game-thread entry.
///
/// The caller continues normal frame work unless this returns true. Time only bounds work; it
/// never proves native quiescence. Ready is validated and consumed here without a frame gap.
pub(crate) fn advance_shutdown(shutting_down: &AtomicBool) -> bool {
    advance_shutdown_state(&SHUTDOWN, shutting_down)
}

/// Fatal initialization or update failure is not a recoverable shutdown deferral.
/// No cleanup or hook removal is attempted without the ordinary preparation prerequisite.
pub(crate) fn mark_fatal() -> bool {
    SHUTDOWN.lock().mark_fatal()
}

pub(crate) fn is_fatal() -> bool {
    matches!(SHUTDOWN.lock().phase, Phase::Fatal)
}

/// Register a one-shot cleanup. These are not preparation participants and cannot veto teardown.
pub fn on_cleanup(cleanup: impl FnOnce(Option<&mut egui_directx11::Renderer>) + Send + 'static) {
    CLEANUPS.lock().push(Box::new(cleanup));
}

/// Drain cleanups once, in reverse registration order, only after the commit boundary.
pub(crate) fn run_cleanups(renderer: Option<&mut egui_directx11::Renderer>) {
    assert!(
        crate::is_shutting_down(),
        "cleanup requires committed shutdown"
    );
    run_cleanup_registry(&CLEANUPS, renderer);
}

type Cleanup = Box<dyn FnOnce(Option<&mut egui_directx11::Renderer>) + Send>;

fn run_cleanup_registry(
    registry: &Mutex<Vec<Cleanup>>,
    renderer: Option<&mut egui_directx11::Renderer>,
) {
    let cleanups = std::mem::take(&mut *registry.lock());
    let mut renderer = renderer;
    for cleanup in cleanups.into_iter().rev() {
        cleanup(renderer.as_deref_mut());
    }
}

fn advance_shutdown_state(shutdown: &Mutex<Shutdown>, shutting_down: &AtomicBool) -> bool {
    let Some(attempt) = shutdown.lock().begin_frame() else {
        return false;
    };
    let prepared = match attempt.prepare() {
        PrepareOutcome::Ready(prepared) => prepared,
        PrepareOutcome::Deferred(reason) => {
            let blocked = Blocked::Deferred(reason);
            let accepted = shutdown.lock().finish_attempt(attempt.preparation, blocked);
            if accepted && attempt.previous != Some(blocked) {
                tracing::info!(
                    ?reason,
                    "shutdown deferred; gameplay and cleanup owners remain live"
                );
            }
            return false;
        }
        PrepareOutcome::Failed(error) => {
            let blocked = Blocked::Failed(error);
            let accepted = shutdown.lock().finish_attempt(attempt.preparation, blocked);
            if accepted && attempt.previous != Some(blocked) {
                tracing::error!(%error, "shutdown preparation failed; teardown has not begun");
            }
            return false;
        }
    };
    let committed = commit_prepared(shutdown, prepared, shutting_down);
    if !committed {
        shutdown.lock().finish_attempt(
            attempt.preparation,
            Blocked::Deferred(DeferReason::StalePreparation),
        );
    }
    committed
}

const RETRY_FRAMES: u8 = 15;
static CLEANUPS: Mutex<Vec<Cleanup>> = Mutex::new(Vec::new());
static SHUTDOWN: Mutex<Shutdown> = Mutex::new(Shutdown::new());

struct Shutdown {
    phase: Phase,
    request_generation: u64,
    attempt_generation: u64,
    revision: u64,
    participants: [Option<Arc<dyn ShutdownParticipant>>; 2],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Phase {
    Live,
    Pending { wait: u8, blocked: Option<Blocked> },
    Preparing(Preparation),
    Committed,
    Fatal,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Blocked {
    Deferred(DeferReason),
    Failed(PrepareError),
}

impl Shutdown {
    const fn new() -> Self {
        Self {
            phase: Phase::Live,
            request_generation: 0,
            attempt_generation: 0,
            revision: 0,
            participants: [None, None],
        }
    }

    fn register(
        &mut self,
        id: ParticipantId,
        participant: Arc<dyn ShutdownParticipant>,
    ) -> Result<(), RegistrationError> {
        if !matches!(self.phase, Phase::Live) {
            return Err(RegistrationError::ShutdownStarted);
        }
        let index = match id {
            ParticipantId::MagazineExecutor => 0,
            ParticipantId::HumanCollider => 1,
        };
        if self.participants[index].is_some() {
            return Err(RegistrationError::Duplicate(id));
        }
        self.participants[index] = Some(participant);
        self.revision = self
            .revision
            .checked_add(1)
            .expect("participant generation exhausted");
        Ok(())
    }

    fn request(&mut self) -> bool {
        if !matches!(self.phase, Phase::Live) {
            return false;
        }
        self.request_generation = self
            .request_generation
            .checked_add(1)
            .expect("shutdown generation exhausted");
        self.phase = Phase::Pending {
            wait: 0,
            blocked: None,
        };
        true
    }

    fn begin_frame(&mut self) -> Option<Attempt> {
        let Phase::Pending { wait, blocked } = self.phase else {
            return None;
        };
        if wait != 0 {
            self.phase = Phase::Pending {
                wait: wait - 1,
                blocked,
            };
            return None;
        }
        self.attempt_generation = self
            .attempt_generation
            .checked_add(1)
            .expect("preparation generation exhausted");
        let preparation = Preparation {
            request: self.request_generation,
            attempt: self.attempt_generation,
            revision: self.revision,
        };
        self.phase = Phase::Preparing(preparation);
        Some(Attempt {
            preparation,
            participants: self.participants.clone(),
            previous: blocked,
        })
    }

    fn finish_attempt(&mut self, preparation: Preparation, blocked: Blocked) -> bool {
        if self.phase != Phase::Preparing(preparation) {
            return false;
        }
        self.phase = Phase::Pending {
            wait: RETRY_FRAMES - 1,
            blocked: Some(blocked),
        };
        true
    }

    fn mark_fatal(&mut self) -> bool {
        if matches!(self.phase, Phase::Fatal | Phase::Committed) {
            return false;
        }
        self.phase = Phase::Fatal;
        true
    }
}

fn commit_prepared(
    shutdown: &Mutex<Shutdown>,
    mut prepared: PreparedShutdown,
    shutting_down: &AtomicBool,
) -> bool {
    // Validation may query the engine. The guards retain their barriers across this lock gap.
    if !prepared
        .participants
        .iter()
        .all(|p| p.validate(prepared.preparation))
    {
        return false;
    }
    let preparation = prepared.preparation;
    let mut state = shutdown.lock();
    if state.phase != Phase::Preparing(preparation) || preparation.revision != state.revision {
        drop(state);
        return false;
    }
    // Finalizers cannot query the engine, re-enter lifecycle, fail, or release their barriers.
    for participant in &mut prepared.participants {
        participant.commit();
    }
    state.phase = Phase::Committed;
    shutting_down.store(true, Ordering::Release);
    drop(state);
    true
}

struct Attempt {
    preparation: Preparation,
    participants: [Option<Arc<dyn ShutdownParticipant>>; 2],
    previous: Option<Blocked>,
}

impl Attempt {
    fn prepare(&self) -> PrepareOutcome<PreparedShutdown> {
        let mut prepared = PreparedShutdown {
            preparation: self.preparation,
            participants: Vec::new(),
        };
        for participant in self.participants.iter().flatten() {
            match participant.prepare(self.preparation) {
                PrepareOutcome::Ready(guard) => prepared.participants.push(guard),
                PrepareOutcome::Deferred(reason) => return PrepareOutcome::Deferred(reason),
                PrepareOutcome::Failed(error) => return PrepareOutcome::Failed(error),
            }
        }
        PrepareOutcome::Ready(prepared)
    }
}

struct PreparedShutdown {
    preparation: Preparation,
    participants: Vec<Box<dyn PreparedParticipant>>,
}

impl Drop for PreparedShutdown {
    fn drop(&mut self) {
        while let Some(participant) = self.participants.pop() {
            drop(participant);
        }
    }
}

#[cfg(test)]
mod tests;
