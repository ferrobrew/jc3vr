use std::{
    collections::VecDeque,
    sync::{
        Weak,
        atomic::{AtomicU64, AtomicUsize},
    },
};

use super::*;

#[test]
fn repeated_requests_defer_without_stopping_frames_or_draining_cleanups() {
    let harness = Harness::new();
    let magazine = FakeParticipant::new(ParticipantId::MagazineExecutor, [Answer::Deferred]);
    harness.register(&magazine);
    harness.add_cleanup("cleanup");

    for _ in 0..u16::from(RETRY_FRAMES) * 3 {
        harness.shutdown.lock().request();
        assert!(!harness.frame());
        assert!(!harness.shutting_down.load(Ordering::Acquire));
        assert_eq!(harness.cleanups.lock().len(), 1);
        assert!(harness.events.lock().is_empty());
    }

    assert_eq!(magazine.calls.load(Ordering::Relaxed), 3);
    assert_eq!(harness.shutdown.lock().request_generation, 1);
    assert_eq!(
        harness.normal_frames.load(Ordering::Relaxed),
        usize::from(RETRY_FRAMES) * 3
    );
    assert_eq!(magazine.commits.load(Ordering::Relaxed), 0);
}

#[test]
fn partial_failure_aborts_prepared_guard_and_retries_before_one_shot_cleanup() {
    let harness = Harness::new();
    let magazine = FakeParticipant::new(ParticipantId::MagazineExecutor, [Answer::Ready]);
    let collider = FakeParticipant::new(
        ParticipantId::HumanCollider,
        [Answer::Failed, Answer::Ready],
    );
    harness.register(&magazine);
    harness.register(&collider);
    harness.add_cleanup("first");
    harness.add_cleanup("second");
    harness.shutdown.lock().request();

    assert!(!harness.frame());
    assert_eq!(magazine.aborts.load(Ordering::Relaxed), 1);
    assert_eq!(magazine.commits.load(Ordering::Relaxed), 0);
    assert_eq!(harness.cleanups.lock().len(), 2);
    assert!(!harness.shutting_down.load(Ordering::Acquire));

    for _ in 1..RETRY_FRAMES {
        assert!(!harness.frame());
    }
    assert!(harness.frame());
    assert_eq!(magazine.commits.load(Ordering::Relaxed), 1);
    assert_eq!(collider.commits.load(Ordering::Relaxed), 1);
    assert_eq!(
        harness.events.lock().as_slice(),
        &["commit", "second", "first"]
    );
    assert_eq!(harness.cleanup_calls.load(Ordering::Relaxed), 2);
    assert!(harness.shutting_down.load(Ordering::Acquire));

    assert!(!harness.shutdown.lock().request());
    for _ in 0..RETRY_FRAMES {
        assert!(!harness.frame());
    }
    run_cleanup_registry(&harness.cleanups, None);
    assert_eq!(harness.cleanup_calls.load(Ordering::Relaxed), 2);
    assert_eq!(magazine.commits.load(Ordering::Relaxed), 1);
}

#[test]
fn partial_defer_rolls_back_an_earlier_prepared_participant() {
    let harness = Harness::new();
    let magazine = FakeParticipant::new(ParticipantId::MagazineExecutor, [Answer::Ready]);
    let collider = FakeParticipant::new(ParticipantId::HumanCollider, [Answer::Deferred]);
    harness.register(&magazine);
    harness.register(&collider);
    harness.add_cleanup("cleanup");
    harness.shutdown.lock().request();

    assert!(!harness.frame());
    assert_eq!(magazine.aborts.load(Ordering::Relaxed), 1);
    assert_eq!(magazine.commits.load(Ordering::Relaxed), 0);
    assert!(!harness.shutting_down.load(Ordering::Acquire));
    assert_eq!(harness.cleanups.lock().len(), 1);
}

#[test]
fn stale_attempt_cannot_commit_during_a_later_attempt() {
    let harness = Harness::new();
    let participant = FakeParticipant::new(ParticipantId::MagazineExecutor, [Answer::Ready]);
    harness.register(&participant);
    harness.shutdown.lock().request();
    let first = harness.shutdown.lock().begin_frame().unwrap();
    let stale = ready(first.prepare());
    assert!(harness.shutdown.lock().finish_attempt(
        first.preparation,
        Blocked::Deferred(DeferReason::MagazineBusy),
    ));
    for _ in 1..RETRY_FRAMES {
        assert!(harness.shutdown.lock().begin_frame().is_none());
    }
    let second = harness.shutdown.lock().begin_frame().unwrap();
    assert_ne!(first.preparation, second.preparation);
    assert!(!commit_prepared(
        &harness.shutdown,
        stale,
        &harness.shutting_down
    ));
    assert!(!harness.shutting_down.load(Ordering::Acquire));
    assert_eq!(participant.aborts.load(Ordering::Relaxed), 1);
    assert!(commit_prepared(
        &harness.shutdown,
        ready(second.prepare()),
        &harness.shutting_down
    ));
    assert_eq!(participant.commits.load(Ordering::Relaxed), 1);
}

#[test]
fn a_previous_request_token_cannot_authorize_the_current_request() {
    let harness = Harness::new();
    harness.shutdown.lock().request();
    let attempt = harness.shutdown.lock().begin_frame().unwrap();
    let mut prepared = ready(attempt.prepare());
    prepared.preparation.request = prepared.preparation.request.saturating_sub(1);
    assert!(!commit_prepared(
        &harness.shutdown,
        prepared,
        &harness.shutting_down
    ));
    assert!(!harness.shutting_down.load(Ordering::Acquire));
}

#[test]
fn changed_participant_generation_rejects_preparation_and_allows_fresh_retry() {
    let harness = Harness::new();
    let participant = FakeParticipant::new(ParticipantId::HumanCollider, [Answer::Ready]);
    harness.register(&participant);
    harness.shutdown.lock().request();
    let attempt = harness.shutdown.lock().begin_frame().unwrap();
    let prepared = ready(attempt.prepare());
    participant.epoch.fetch_add(1, Ordering::Release);

    assert!(!commit_prepared(
        &harness.shutdown,
        prepared,
        &harness.shutting_down
    ));
    assert!(!harness.shutting_down.load(Ordering::Acquire));
    assert_eq!(participant.aborts.load(Ordering::Relaxed), 1);
    harness.shutdown.lock().finish_attempt(
        attempt.preparation,
        Blocked::Deferred(DeferReason::StalePreparation),
    );
    for _ in 1..RETRY_FRAMES {
        assert!(!harness.frame());
    }
    assert!(harness.frame());
    assert_eq!(participant.commits.load(Ordering::Relaxed), 1);
}

#[test]
fn changed_registry_revision_invalidates_an_existing_token() {
    let harness = Harness::new();
    harness.shutdown.lock().request();
    let attempt = harness.shutdown.lock().begin_frame().unwrap();
    let prepared = ready(attempt.prepare());
    harness.shutdown.lock().revision += 1;
    assert!(!commit_prepared(
        &harness.shutdown,
        prepared,
        &harness.shutting_down
    ));
    assert!(!harness.shutting_down.load(Ordering::Acquire));
}

#[test]
fn fatal_failure_is_not_live_deferral_and_cannot_authorize_cleanup() {
    let harness = Harness::new();
    harness.add_cleanup("cleanup");
    harness.shutdown.lock().request();
    let attempt = harness.shutdown.lock().begin_frame().unwrap();
    let prepared = ready(attempt.prepare());
    assert!(harness.shutdown.lock().mark_fatal());
    assert!(!harness.shutdown.lock().mark_fatal());
    assert!(!harness.shutdown.lock().request());
    assert!(!commit_prepared(
        &harness.shutdown,
        prepared,
        &harness.shutting_down
    ));
    assert!(!harness.shutting_down.load(Ordering::Acquire));
    assert_eq!(harness.cleanups.lock().len(), 1);
    assert_eq!(harness.shutdown.lock().phase, Phase::Fatal);
}

#[test]
fn registration_is_exclusive_and_closes_when_shutdown_is_requested() {
    let harness = Harness::new();
    let participant = FakeParticipant::new(ParticipantId::MagazineExecutor, [Answer::Ready]);
    harness.register(&participant);
    assert_eq!(
        harness
            .shutdown
            .lock()
            .register(ParticipantId::MagazineExecutor, participant),
        Err(RegistrationError::Duplicate(
            ParticipantId::MagazineExecutor
        )),
    );
    harness.shutdown.lock().request();
    assert_eq!(
        harness.shutdown.lock().register(
            ParticipantId::HumanCollider,
            FakeParticipant::new(ParticipantId::HumanCollider, [Answer::Ready]),
        ),
        Err(RegistrationError::ShutdownStarted),
    );
}

#[test]
fn preparation_and_validation_do_not_hold_the_lifecycle_mutex() {
    let shutdown = Arc::new(Mutex::new(Shutdown::new()));
    shutdown
        .lock()
        .register(
            ParticipantId::HumanCollider,
            Arc::new(LockCheckingParticipant {
                shutdown: Arc::downgrade(&shutdown),
            }),
        )
        .unwrap();
    shutdown.lock().request();
    assert!(advance_shutdown_state(&shutdown, &AtomicBool::new(false)));
}

#[test]
fn magazine_activation_refuses_an_unproven_entry_exclusion_contract() {
    assert_eq!(
        crate::hooks::activate_magazine_executor(),
        Err(DeferReason::HookEntryExclusionUnproven),
    );
}

struct Harness {
    shutdown: Mutex<Shutdown>,
    shutting_down: AtomicBool,
    normal_frames: AtomicUsize,
    cleanups: Mutex<Vec<Cleanup>>,
    cleanup_calls: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl Harness {
    fn new() -> Self {
        Self {
            shutdown: Mutex::new(Shutdown::new()),
            shutting_down: AtomicBool::new(false),
            normal_frames: AtomicUsize::new(0),
            cleanups: Mutex::new(Vec::new()),
            cleanup_calls: Arc::new(AtomicUsize::new(0)),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn register(&self, participant: &Arc<FakeParticipant>) {
        self.shutdown
            .lock()
            .register(participant.id, participant.clone())
            .unwrap();
    }

    fn add_cleanup(&self, label: &'static str) {
        let calls = self.cleanup_calls.clone();
        let events = self.events.clone();
        self.cleanups.lock().push(Box::new(move |renderer| {
            assert!(renderer.is_none());
            calls.fetch_add(1, Ordering::Relaxed);
            events.lock().push(label);
        }));
    }

    fn frame(&self) -> bool {
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
enum Answer {
    Ready,
    Deferred,
    Failed,
}

struct FakeParticipant {
    id: ParticipantId,
    answers: Mutex<VecDeque<Answer>>,
    calls: AtomicUsize,
    commits: Arc<AtomicUsize>,
    aborts: Arc<AtomicUsize>,
    epoch: Arc<AtomicU64>,
}

impl FakeParticipant {
    fn new(id: ParticipantId, answers: impl IntoIterator<Item = Answer>) -> Arc<Self> {
        Arc::new(Self {
            id,
            answers: Mutex::new(answers.into_iter().collect()),
            calls: AtomicUsize::new(0),
            commits: Arc::new(AtomicUsize::new(0)),
            aborts: Arc::new(AtomicUsize::new(0)),
            epoch: Arc::new(AtomicU64::new(1)),
        })
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

struct LockCheckingParticipant {
    shutdown: Weak<Mutex<Shutdown>>,
}

impl ShutdownParticipant for LockCheckingParticipant {
    fn prepare(&self, preparation: Preparation) -> PrepareOutcome<Box<dyn PreparedParticipant>> {
        let shutdown = self.shutdown.upgrade().unwrap();
        assert!(shutdown.try_lock().is_some());
        PrepareOutcome::Ready(Box::new(LockCheckingGuard {
            shutdown: self.shutdown.clone(),
            preparation,
        }))
    }
}

struct LockCheckingGuard {
    shutdown: Weak<Mutex<Shutdown>>,
    preparation: Preparation,
}

impl PreparedParticipant for LockCheckingGuard {
    fn validate(&self, preparation: Preparation) -> bool {
        let shutdown = self.shutdown.upgrade().unwrap();
        assert!(shutdown.try_lock().is_some());
        self.preparation == preparation
    }

    fn commit(&mut self) {}
}

fn ready(outcome: PrepareOutcome<PreparedShutdown>) -> PreparedShutdown {
    match outcome {
        PrepareOutcome::Ready(prepared) => prepared,
        _ => panic!("expected prepared shutdown"),
    }
}
