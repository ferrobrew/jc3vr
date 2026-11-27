use std::sync::Weak;

use super::*;
use crate::lifecycle::test_support::{Answer, FakeParticipant, Harness};

const FIRST: ParticipantId = ParticipantId::new("the first participant");
const SECOND: ParticipantId = ParticipantId::new("the second participant");

#[test]
fn repeated_requests_defer_without_stopping_frames_or_draining_cleanups() {
    let harness = Harness::new();
    let first = FakeParticipant::new(FIRST, [Answer::Deferred]);
    harness.register(&first);
    harness.add_cleanup("cleanup");

    for _ in 0..u16::from(RETRY_FRAMES) * 3 {
        harness.shutdown.lock().request();
        assert!(!harness.frame());
        assert!(!harness.shutting_down.load(Ordering::Acquire));
        assert_eq!(harness.cleanups.lock().len(), 1);
        assert!(harness.events.lock().is_empty());
    }

    assert_eq!(first.calls.load(Ordering::Relaxed), 3);
    assert_eq!(harness.shutdown.lock().request_generation, 1);
    assert_eq!(
        harness.normal_frames.load(Ordering::Relaxed),
        usize::from(RETRY_FRAMES) * 3
    );
    assert_eq!(first.commits.load(Ordering::Relaxed), 0);
}

#[test]
fn partial_failure_aborts_prepared_guard_and_retries_before_one_shot_cleanup() {
    let harness = Harness::new();
    let first = FakeParticipant::new(FIRST, [Answer::Ready]);
    let second = FakeParticipant::new(SECOND, [Answer::Failed, Answer::Ready]);
    harness.register(&first);
    harness.register(&second);
    harness.add_cleanup("first");
    harness.add_cleanup("second");
    harness.shutdown.lock().request();

    assert!(!harness.frame());
    assert_eq!(first.aborts.load(Ordering::Relaxed), 1);
    assert_eq!(first.commits.load(Ordering::Relaxed), 0);
    assert_eq!(harness.cleanups.lock().len(), 2);
    assert!(!harness.shutting_down.load(Ordering::Acquire));

    for _ in 1..RETRY_FRAMES {
        assert!(!harness.frame());
    }
    assert!(harness.frame());
    assert_eq!(first.commits.load(Ordering::Relaxed), 1);
    assert_eq!(second.commits.load(Ordering::Relaxed), 1);
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
    assert_eq!(first.commits.load(Ordering::Relaxed), 1);
}

#[test]
fn partial_defer_rolls_back_an_earlier_prepared_participant() {
    let harness = Harness::new();
    let first = FakeParticipant::new(FIRST, [Answer::Ready]);
    let second = FakeParticipant::new(SECOND, [Answer::Deferred]);
    harness.register(&first);
    harness.register(&second);
    harness.add_cleanup("cleanup");
    harness.shutdown.lock().request();

    assert!(!harness.frame());
    assert_eq!(first.aborts.load(Ordering::Relaxed), 1);
    assert_eq!(first.commits.load(Ordering::Relaxed), 0);
    assert!(!harness.shutting_down.load(Ordering::Acquire));
    assert_eq!(harness.cleanups.lock().len(), 1);
}

#[test]
fn stale_attempt_cannot_commit_during_a_later_attempt() {
    let harness = Harness::new();
    let participant = FakeParticipant::new(FIRST, [Answer::Ready]);
    harness.register(&participant);
    harness.shutdown.lock().request();
    let first = harness.shutdown.lock().begin_frame().unwrap();
    let stale = ready(first.prepare());
    assert!(harness.shutdown.lock().finish_attempt(
        first.preparation,
        Blocked::Deferred(DeferReason::Busy {
            participant: FIRST,
            cause: "busy",
        }),
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
    let participant = FakeParticipant::new(SECOND, [Answer::Ready]);
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
    let participant = FakeParticipant::new(FIRST, [Answer::Ready]);
    harness.register(&participant);
    assert_eq!(
        harness.shutdown.lock().register(FIRST, participant),
        Err(RegistrationError::Duplicate(FIRST)),
    );
    harness.shutdown.lock().request();
    assert_eq!(
        harness
            .shutdown
            .lock()
            .register(SECOND, FakeParticipant::new(SECOND, [Answer::Ready]),),
        Err(RegistrationError::ShutdownStarted),
    );
}

#[test]
fn preparation_and_validation_do_not_hold_the_lifecycle_mutex() {
    let shutdown = Arc::new(Mutex::new(Shutdown::new()));
    shutdown
        .lock()
        .register(
            SECOND,
            Arc::new(LockCheckingParticipant {
                shutdown: Arc::downgrade(&shutdown),
            }),
        )
        .unwrap();
    shutdown.lock().request();
    assert!(advance_shutdown_state(&shutdown, &AtomicBool::new(false)));
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
