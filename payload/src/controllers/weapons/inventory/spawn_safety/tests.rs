use std::sync::mpsc;

use super::*;

#[test]
fn admission_stays_closed_until_installation_activates_it() {
    let safety = SpawnSafety::new();
    assert_eq!(
        safety.submit(RequestKind::Restore, || panic!("native call")),
        Err(AdmissionClosed)
    );
    safety.activate();
    assert_eq!(safety.submit(RequestKind::Restore, || 17), Ok(17));
}

#[test]
fn either_request_kind_latches_exposure_before_the_native_boundary_without_vetoing_unload() {
    for kind in [RequestKind::Restore, RequestKind::Twin] {
        let safety = SpawnSafety::new();
        safety.activate();
        safety
            .submit(kind, || {
                let state = safety
                    .owner
                    .state
                    .try_lock()
                    .expect("no lock across native call");
                assert_eq!(state.first_exposure, Some(kind));
                assert_eq!(state.submissions_in_flight, 1);
                drop(state);
                assert_deferred(&safety, DeferReason::SpawnCallbackActive);
            })
            .unwrap();
        assert_eq!(safety.owner.state.lock().submissions_in_flight, 0);
        assert_eq!(safety.submit(kind, || 19), Ok(19));
        let preparation = Preparation::for_test();
        let mut prepared = ready(safety.prepare(preparation));
        assert!(prepared.validate(preparation));
        prepared.commit();
        drop(prepared);
        assert_eq!(safety.owner.state.lock().first_exposure, Some(kind));
        assert!(safety.post_unpatch_check().is_ok());
    }
}

#[test]
fn no_exposure_preparation_excludes_submission_through_commit() {
    let safety = SpawnSafety::new();
    safety.activate();
    let preparation = Preparation::for_test();
    let mut prepared = ready(safety.prepare(preparation));
    assert!(prepared.validate(preparation));
    assert_eq!(
        safety.submit(RequestKind::Twin, || panic!("native call")),
        Err(AdmissionClosed)
    );
    safety.activate();
    assert!(prepared.validate(preparation));
    prepared.commit();
    drop(prepared);
    safety.activate();
    assert_eq!(
        safety.submit(RequestKind::Restore, || panic!("native call")),
        Err(AdmissionClosed)
    );
    assert!(safety.post_unpatch_check().is_ok());
}

#[test]
fn abort_restores_the_previous_admission_without_exposure() {
    let safety = SpawnSafety::new();
    safety.activate();
    let preparation = Preparation::for_test();
    drop(ready(safety.prepare(preparation)));
    assert_eq!(safety.submit(RequestKind::Twin, || 23), Ok(23));
    drop(ready(safety.prepare(preparation)));
}

#[test]
fn submission_racing_preparation_never_crosses_native_after_exclusion() {
    let safety = Arc::new(SpawnSafety::new());
    safety.activate();
    let preparation = Preparation::for_test();
    let prepared = ready(safety.prepare(preparation));
    let submitting = Arc::clone(&safety);
    let rejected = std::thread::spawn(move || {
        submitting.submit(RequestKind::Restore, || panic!("native call after prepare"))
    });
    assert_eq!(rejected.join().unwrap(), Err(AdmissionClosed));
    assert!(prepared.validate(preparation));
    drop(prepared);
    assert_eq!(safety.submit(RequestKind::Restore, || 29), Ok(29));
}

#[test]
fn submission_in_flight_defers_without_waiting_or_disabling_gameplay() {
    let safety = Arc::new(SpawnSafety::new());
    safety.activate();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let submitting = Arc::clone(&safety);
    let thread = std::thread::spawn(move || {
        submitting.submit(RequestKind::Twin, || {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        })
    });
    entered_rx.recv().unwrap();
    assert_deferred(&safety, DeferReason::SpawnCallbackActive);
    let blocked = safety.post_unpatch_check().unwrap_err();
    assert_eq!(blocked.submissions_in_flight, 1);
    assert_eq!(blocked.admission, Admission::Open);
    release_tx.send(()).unwrap();
    thread.join().unwrap().unwrap();
    assert_eq!(safety.submit(RequestKind::Restore, || 31), Ok(31));
}

#[test]
fn callback_during_preparation_invalidates_it_only_while_running() {
    let safety = SpawnSafety::new();
    safety.activate();
    let preparation = Preparation::for_test();
    let mut prepared = ready(safety.prepare(preparation));
    let callback = safety.callback_entry();
    assert!(!prepared.validate(preparation));
    let blocked = safety.post_unpatch_check().unwrap_err();
    assert_eq!(blocked.active_callbacks, 1);
    assert!(blocked.callback_entered);
    drop(callback);
    assert!(prepared.validate(preparation));
    prepared.commit();
    drop(prepared);
    assert!(safety.post_unpatch_check().is_ok());
}

#[test]
fn returned_callbacks_and_silent_submissions_warn_but_permit_unload() {
    let safety = SpawnSafety::new();
    safety.activate();
    // SpawnTagIndex returns no acknowledgement, including when no factory is queued.
    safety.submit(RequestKind::Restore, || ()).unwrap();
    let callback = safety.callback_entry();
    assert_deferred(&safety, DeferReason::SpawnCallbackActive);
    drop(callback);
    assert_eq!(safety.owner.active_callbacks.load(Ordering::Acquire), 0);
    // Without a committed preparation, the post-unpatch check still refuses.
    assert!(safety.post_unpatch_check().is_err());
    let preparation = Preparation::for_test();
    let mut prepared = ready(safety.prepare(preparation));
    assert!(prepared.validate(preparation));
    prepared.commit();
    drop(prepared);
    assert!(safety.post_unpatch_check().is_ok());
}

fn assert_deferred(safety: &SpawnSafety, expected: DeferReason) {
    match safety.prepare(Preparation::for_test()) {
        PrepareOutcome::Deferred(reason) => assert_eq!(reason, expected),
        _ => panic!("expected deferred preparation"),
    }
}

fn ready(outcome: PrepareOutcome<Box<dyn PreparedParticipant>>) -> Box<dyn PreparedParticipant> {
    match outcome {
        PrepareOutcome::Ready(prepared) => prepared,
        _ => panic!("expected ready preparation"),
    }
}
