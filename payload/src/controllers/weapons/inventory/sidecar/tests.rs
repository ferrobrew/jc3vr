use std::{
    fs,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
};

use super::*;

#[test]
fn stalled_writer_coalesces_latest_sequence_and_retains_owner_on_shutdown_timeout() {
    let owner = Mutex::new(SidecarWriter::new());
    let (entered, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let writes = Arc::new(Mutex::new(Vec::new()));
    let written = Arc::clone(&writes);
    let publications = Arc::new(AtomicUsize::new(0));
    let published = Arc::clone(&publications);
    owner
        .lock()
        .submit_with(
            snapshot(1),
            move |snapshot| {
                written.lock().push(snapshot.sequence);
                entered.send(snapshot.sequence).unwrap();
                if snapshot.sequence == 1 {
                    wait.recv().unwrap();
                }
                Ok(())
            },
            move || {
                published.fetch_add(1, Ordering::Relaxed);
            },
        )
        .unwrap();
    assert_eq!(ready.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
    let thread = owner.lock().handle.as_ref().unwrap().thread().id();
    for sequence in 2..=100 {
        owner
            .lock()
            .submit_with(snapshot(sequence), |_| panic!("must reuse worker"), || {})
            .unwrap();
    }
    owner
        .lock()
        .submit_with(
            snapshot(50),
            |_| panic!("older snapshot must be ignored"),
            || {},
        )
        .unwrap();
    {
        let owner = owner.lock();
        assert_eq!(owner.handle.as_ref().unwrap().thread().id(), thread);
        let queue = owner.shared.as_ref().unwrap().queue.lock();
        assert_eq!(queue.pending.as_ref().unwrap().sequence, 100);
        assert_eq!(queue.latest, 100);
        assert_eq!(queue.status.writes, 0);
    }
    assert!(!shutdown_owner(&owner, Duration::ZERO));
    assert!(owner.lock().handle.is_some());
    assert!(
        owner
            .lock()
            .submit_with(snapshot(101), |_| Ok(()), || {})
            .is_err()
    );
    release.send(()).unwrap();
    assert!(shutdown_owner(&owner, Duration::from_secs(5)));
    assert_eq!(*writes.lock(), [1, 100]);
    let status = owner.lock().status();
    assert_eq!(status.sequence, 100);
    assert_eq!(status.writes, 2);
    assert_eq!(status.last_written, 100);
    assert!(status.error.is_none());
    assert_eq!(publications.load(Ordering::Relaxed), 2);
    assert!(owner.lock().handle.is_none());
    assert!(!owner.lock().reap());
    assert!(shutdown_owner(&owner, Duration::ZERO));
    assert_eq!(publications.load(Ordering::Relaxed), 2);
}

#[test]
fn failed_and_panicked_attempts_do_not_claim_saved_and_newest_snapshot_recovers() {
    let owner = Mutex::new(SidecarWriter::new());
    let (published, receive) = mpsc::channel();
    owner
        .lock()
        .submit_with(
            snapshot(1),
            |snapshot| match snapshot.sequence {
                1 => Err("inventory: sidecar: controlled failure".into()),
                2 => panic!("controlled write panic"),
                _ => Ok(()),
            },
            move || {
                published.send(()).unwrap();
            },
        )
        .unwrap();
    receive.recv_timeout(Duration::from_secs(5)).unwrap();
    let status = owner.lock().status();
    assert_eq!(status.writes, 0);
    assert_eq!(status.last_written, 0);
    assert!(status.error.unwrap().contains("controlled failure"));
    owner
        .lock()
        .submit_with(snapshot(2), |_| panic!("must reuse worker"), || {})
        .unwrap();
    receive.recv_timeout(Duration::from_secs(5)).unwrap();
    let status = owner.lock().status();
    assert_eq!(status.sequence, 2);
    assert_eq!(status.writes, 0);
    assert!(status.error.unwrap().contains("panicked"));
    owner
        .lock()
        .submit_with(snapshot(3), |_| panic!("must reuse worker"), || {})
        .unwrap();
    receive.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(shutdown_owner(&owner, Duration::from_secs(5)));
    let status = owner.lock().status();
    assert_eq!(status.sequence, 3);
    assert_eq!(status.writes, 1);
    assert_eq!(status.last_written, 3);
    assert!(status.error.is_none());
    assert!(!owner.lock().reap());
}

#[test]
fn publication_must_return_and_thread_join_before_shutdown_permits_unload() {
    let owner = Mutex::new(SidecarWriter::new());
    let (entered, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let callbacks = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&callbacks);
    owner
        .lock()
        .submit_with(
            snapshot(1),
            |_| Ok(()),
            move || {
                called.fetch_add(1, Ordering::Relaxed);
                entered.send(()).unwrap();
                wait.recv().unwrap();
            },
        )
        .unwrap();
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!shutdown_owner(&owner, Duration::ZERO));
    assert!(owner.lock().handle.is_some());
    release.send(()).unwrap();
    assert!(shutdown_owner(&owner, Duration::from_secs(5)));
    assert_eq!(callbacks.load(Ordering::Relaxed), 1);
    assert!(!owner.lock().reap());
    assert!(
        owner
            .lock()
            .submit_with(snapshot(2), |_| Ok(()), || panic!("closed"))
            .is_err()
    );
    assert_eq!(callbacks.load(Ordering::Relaxed), 1);
}

#[test]
fn unexpected_thread_panic_is_joined_and_reported_once() {
    let mut owner = SidecarWriter::new();
    owner
        .submit_with(
            snapshot(1),
            |_| Ok(()),
            || panic!("controlled publication panic"),
        )
        .unwrap();
    let owner = Mutex::new(owner);
    assert!(shutdown_owner(&owner, Duration::from_secs(5)));
    assert!(
        owner
            .lock()
            .status()
            .error
            .unwrap()
            .contains("thread panicked")
    );
    assert!(owner.lock().handle.is_none());
    assert!(!owner.lock().reap());
}

#[test]
fn panic_with_newest_pending_snapshot_does_not_report_drained() {
    let owner = Mutex::new(SidecarWriter::new());
    let (entered, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    owner
        .lock()
        .submit_with(
            snapshot(1),
            |_| Ok(()),
            move || {
                entered.send(()).unwrap();
                wait.recv().unwrap();
                panic!("controlled publication panic with newer pending snapshot");
            },
        )
        .unwrap();
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    owner
        .lock()
        .submit_with(snapshot(2), |_| Ok(()), || {})
        .unwrap();
    release.send(()).unwrap();
    assert!(!shutdown_owner(&owner, Duration::from_secs(5)));
    let owner = owner.lock();
    assert!(owner.handle.is_none());
    assert_eq!(
        owner
            .shared
            .as_ref()
            .unwrap()
            .queue
            .lock()
            .pending
            .as_ref()
            .unwrap()
            .sequence,
        2
    );
    assert!(owner.status().error.unwrap().contains("thread panicked"));
}

#[test]
fn actual_atomic_replace_preserves_previous_file_on_failure_and_latest_bytes_on_success() {
    let folder = TempFolder::new();
    let path = folder.0.join("inventory.toml");
    let owner = Mutex::new(SidecarWriter::new());
    let (published, receive) = mpsc::channel();
    let mut first = snapshot(1);
    first.path = path.clone();
    owner
        .lock()
        .submit_with(first, replace, move || {
            published.send(()).unwrap();
        })
        .unwrap();
    receive.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "snapshot-1");
    let mut failed = snapshot(2);
    failed.path = folder.0.join("missing").join("inventory.toml");
    owner.lock().submit_with(failed, replace, || {}).unwrap();
    receive.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "snapshot-1");
    assert_eq!(owner.lock().status().writes, 1);
    assert_eq!(owner.lock().status().last_written, 1);
    assert!(owner.lock().status().error.is_some());
    let mut newest = snapshot(3);
    newest.path = path.clone();
    owner.lock().submit_with(newest, replace, || {}).unwrap();
    assert!(shutdown_owner(&owner, Duration::from_secs(5)));
    assert_eq!(fs::read_to_string(&path).unwrap(), "snapshot-3");
    assert_eq!(owner.lock().status().writes, 2);
    assert_eq!(owner.lock().status().last_written, 3);
    assert!(owner.lock().status().error.is_none());
    assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);
}

fn snapshot(sequence: u64) -> Snapshot {
    Snapshot {
        path: PathBuf::from("inventory.toml"),
        text: format!("snapshot-{sequence}"),
        sequence,
        count: sequence as usize,
    }
}

struct TempFolder(PathBuf);

impl TempFolder {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "jc3vr-sidecar-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
