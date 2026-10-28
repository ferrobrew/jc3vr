use std::{sync::mpsc, thread};

use super::*;

#[test]
fn write_status_updates_only_matching_path_and_preserves_success_on_failure() {
    let mut status = RestoreStatus {
        path: Some(PathBuf::from("current.toml")),
        writes: 2,
        last_written: 3,
        ..RestoreStatus::default()
    };
    apply_write_status(
        &mut status,
        sidecar::WriteStatus {
            sequence: 4,
            path: Some(PathBuf::from("other.toml")),
            writes: 9,
            last_written: 9,
            error: Some("inventory: sidecar: older path failure".into()),
        },
    );
    assert_eq!(status.writes, 2);
    assert_eq!(status.last_written, 3);
    assert!(status.last_write_error.is_none());
    apply_write_status(
        &mut status,
        sidecar::WriteStatus {
            sequence: 5,
            path: Some(PathBuf::from("current.toml")),
            writes: 2,
            last_written: 3,
            error: Some("inventory: sidecar: newest attempt failed".into()),
        },
    );
    assert_eq!(status.writes, 2);
    assert_eq!(status.last_written, 3);
    assert!(
        status
            .last_write_error
            .unwrap()
            .contains("newest attempt failed")
    );
}

#[test]
fn ui_query_has_no_path_resolution_file_io_or_owner_access() {
    let source = include_str!("../restore.rs");
    let query = source
        .split("pub(crate) fn peek_status()")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    assert!(query.contains("UI_STATUS.try_lock()"));
    for forbidden in [
        "paths::",
        "fs::",
        "create_dir_all",
        "PLAN",
        "STATUS.lock",
        "publish_status",
    ] {
        assert!(
            !query.contains(forbidden),
            "the UI query contains {forbidden}"
        );
    }
}

#[test]
fn actual_ui_query_never_waits_for_restore_owners_or_resolves_a_path() {
    let previous_publication = UI_STATUS.lock().take();
    let original = STATUS.lock().clone();
    let original_path_observed = PATH_OBSERVED.swap(false, Ordering::AcqRel);
    let original_spawning = SPAWNING.swap(0, Ordering::Relaxed);
    assert!(matches!(
        peek_status(),
        Err(StatusUnavailable::Uninitialized)
    ));
    assert!(!PATH_OBSERVED.load(Ordering::Acquire));

    let expected = RestoreStatus {
        path: Some(PathBuf::from("published-inventory.toml")),
        writes: 4,
        last_write_error: Some("inventory: sidecar: test error".to_owned()),
        last_written: 2,
        last_load: Some(LoadOutcome::Accepted {
            carried: 2,
            unknown: 1,
        }),
        last_respawn: Some(RespawnOutcome {
            origin: Some(Origin::Load),
            requested: 3,
            restored: 1,
            failed: 1,
        }),
        restored: 5,
        spawning: 1,
    };
    *STATUS.lock() = expected.clone();
    SPAWNING.store(expected.spawning, Ordering::Relaxed);
    PATH_OBSERVED.store(true, Ordering::Release);
    publish_status();
    let first = peek_status().unwrap();
    assert_eq!(first.status, expected);
    assert_eq!(first.path, PathStatus::Available);
    assert_eq!(
        status_with_owner_fields(
            STATUS.lock().clone(),
            expected.path.clone(),
            expected.spawning,
        ),
        expected,
    );

    let plan = PLAN.lock();
    let status = STATUS.lock();
    let (send, receive) = mpsc::channel();
    let query = thread::spawn(move || send.send(peek_status()).unwrap());
    let result = receive.recv_timeout(Duration::from_secs(1));
    drop(status);
    drop(plan);
    query.join().unwrap();
    let readout = result
        .expect("the actual UI query must not wait for PLAN or STATUS")
        .unwrap();
    assert!(Arc::ptr_eq(&first, &readout));
    assert_eq!(readout.status, expected);

    let publication = UI_STATUS.lock();
    assert!(matches!(peek_status(), Err(StatusUnavailable::Busy)));
    drop(publication);
    observe_path(None);
    assert_eq!(peek_status().unwrap().path, PathStatus::Unavailable);
    PATH_OBSERVED.store(false, Ordering::Release);
    publish_status();
    assert_eq!(peek_status().unwrap().path, PathStatus::Uninitialized);

    *STATUS.lock() = original;
    PATH_OBSERVED.store(original_path_observed, Ordering::Release);
    SPAWNING.store(original_spawning, Ordering::Relaxed);
    *UI_STATUS.lock() = previous_publication;
    assert_eq!(readout.status, expected);
}
