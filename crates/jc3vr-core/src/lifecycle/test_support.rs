//! A lifecycle of its own for tests: a shutdown state, a cleanup registry, and scripted
//! participants, driven one pre-update entry at a time without touching the process-wide state.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;

use crate::lifecycle::{
    Cleanup, DeferReason, ParticipantId, Preparation, PrepareError, PrepareOutcome,
    PreparedParticipant, Shutdown, ShutdownParticipant, advance_shutdown_state,
    run_cleanup_registry,
};

pub(crate) struct Harness {
    pub(super) shutdown: Mutex<Shutdown>,
    pub(super) shutting_down: AtomicBool,
    pub(super) normal_frames: AtomicUsize,
    pub(super) cleanups: Mutex<Vec<Cleanup>>,
    pub(super) cleanup_calls: Arc<AtomicUsize>,
    pub(super) events: Arc<Mutex<Vec<&'static str>>>,
}

impl Harness {
    pub(crate) fn new() -> Self {
        Self {
            shutdown: Mutex::new(Shutdown::new()),
            shutting_down: AtomicBool::new(false),
            normal_frames: AtomicUsize::new(0),
            cleanups: Mutex::new(Vec::new()),
            cleanup_calls: Arc::new(AtomicUsize::new(0)),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn register(&self, participant: &Arc<FakeParticipant>) {
        self.shutdown
            .lock()
            .register(participant.id, participant.clone())
            .unwrap();
    }

    /// Register an owner the lifecycle does not know about, such as a subsystem's own participant.
    pub(crate) fn register_participant(
        &self,
        id: ParticipantId,
        participant: Arc<dyn ShutdownParticipant>,
    ) {
        self.shutdown.lock().register(id, participant).unwrap();
    }

    pub(crate) fn request(&self) {
        self.shutdown.lock().request();
    }

    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }

    pub(crate) fn cleanup_calls(&self) -> usize {
        self.cleanup_calls.load(Ordering::Relaxed)
    }

    pub(crate) fn pending_cleanups(&self) -> usize {
        self.cleanups.lock().len()
    }

    pub(crate) fn add_cleanup(&self, label: &'static str) {
        let calls = self.cleanup_calls.clone();
        let events = self.events.clone();
        self.cleanups.lock().push(Box::new(move |renderer| {
            assert!(renderer.is_none());
            calls.fetch_add(1, Ordering::Relaxed);
            events.lock().push(label);
        }));
    }

    /// Run one pre-update entry: advance shutdown, and drain the cleanups if it commits.
    pub(crate) fn frame(&self) -> bool {
        if advance_shutdown_state(&self.shutdown, &self.shutting_down) {
            self.events.lock().push("commit");
            run_cleanup_registry(&self.cleanups, None);
            true
        } else {
            self.normal_frames.fetch_add(1, Ordering::Relaxed);
            false
        }
    }
}

#[derive(Copy, Clone)]
pub(crate) enum Answer {
    Ready,
    Deferred,
    Failed,
}

pub(crate) struct FakeParticipant {
    id: ParticipantId,
    answers: Mutex<VecDeque<Answer>>,
    pub(super) calls: AtomicUsize,
    pub(super) commits: Arc<AtomicUsize>,
    pub(super) aborts: Arc<AtomicUsize>,
    pub(super) epoch: Arc<AtomicU64>,
}

impl FakeParticipant {
    pub(crate) fn new(id: ParticipantId, answers: impl IntoIterator<Item = Answer>) -> Arc<Self> {
        Arc::new(Self {
            id,
            answers: Mutex::new(answers.into_iter().collect()),
            calls: AtomicUsize::new(0),
            commits: Arc::new(AtomicUsize::new(0)),
            aborts: Arc::new(AtomicUsize::new(0)),
            epoch: Arc::new(AtomicU64::new(1)),
        })
    }

    pub(crate) fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl ShutdownParticipant for FakeParticipant {
    fn prepare(&self, preparation: Preparation) -> PrepareOutcome<Box<dyn PreparedParticipant>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let answer = {
            let mut answers = self.answers.lock();
            let answer = *answers.front().unwrap();
            if answers.len() > 1 {
                answers.pop_front();
            }
            answer
        };
        match answer {
            Answer::Ready => PrepareOutcome::Ready(Box::new(FakeGuard {
                preparation,
                observed: self.epoch.load(Ordering::Acquire),
                epoch: self.epoch.clone(),
                commits: self.commits.clone(),
                aborts: self.aborts.clone(),
                committed: false,
            })),
            Answer::Deferred => PrepareOutcome::Deferred(DeferReason::ParticipantBusy(self.id)),
            Answer::Failed => PrepareOutcome::Failed(PrepareError {
                participant: self.id,
                operation: "test preparation failed",
            }),
        }
    }
}

struct FakeGuard {
    preparation: Preparation,
    observed: u64,
    epoch: Arc<AtomicU64>,
    commits: Arc<AtomicUsize>,
    aborts: Arc<AtomicUsize>,
    committed: bool,
}

impl PreparedParticipant for FakeGuard {
    fn validate(&self, preparation: Preparation) -> bool {
        self.preparation == preparation && self.observed == self.epoch.load(Ordering::Acquire)
    }

    fn commit(&mut self) {
        assert!(!self.committed);
        self.committed = true;
        self.commits.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for FakeGuard {
    fn drop(&mut self) {
        if !self.committed {
            self.aborts.fetch_add(1, Ordering::Relaxed);
        }
    }
}
