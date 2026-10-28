use std::sync::mpsc;
#[cfg(windows)]
use std::{fs, fs::OpenOptions, os::windows::fs::OpenOptionsExt};

use super::*;
use crate::ui::layout::{Layout, STATE};

#[test]
fn blocked_write_keeps_ui_available_coalesces_and_participates_in_shutdown() {
    let owner = Mutex::new(Writer::new());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (written_tx, written_rx) = mpsc::channel();
    owner
        .lock()
        .submit("layout.json".into(), "1".into(), move |snapshot| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            written_tx
                .send((snapshot.generation, snapshot.text.to_string()))
                .unwrap();
            Ok(())
        })
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    for value in 2..=100 {
        owner
            .lock()
            .submit(
                "layout.json".into(),
                value.to_string().into(),
                |_| unreachable!(),
            )
            .unwrap();
    }
    let layout_guard = STATE
        .try_lock()
        .expect("the render layout lock remains available");
    let context = egui::Context::default();
    let mut layout = Layout::default();
    let mut reads = 0;
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        crate::ui::feature_dock(
            ui,
            &mut layout.dock,
            &mut layout.selection,
            false,
            |ui, _| {
                reads += 1;
                ui.label("Live snapshot");
            },
        );
    });
    assert!(reads > 0);
    drop(layout_guard);
    let shared = Arc::clone(owner.lock().shared.as_ref().unwrap());
    let queue = shared.queue.try_lock().expect("I/O holds no queue lock");
    assert_eq!(queue.pending.as_ref().unwrap().text.as_ref(), "100");
    assert!(queue.ack.saved.is_none());
    drop(queue);
    assert!(!shutdown_owner(&owner, Duration::ZERO));
    assert!(owner.lock().handle.is_some());
    assert!(
        owner
            .lock()
            .submit("layout.json".into(), "101".into(), |_| Ok(()))
            .is_err()
    );
    release_tx.send(()).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    release_tx.send(()).unwrap();
    assert!(shutdown_owner(&owner, Duration::from_secs(5)));
    assert_eq!(
        written_rx.try_iter().collect::<Vec<_>>(),
        [(1, "1".into()), (100, "100".into())]
    );
    assert_eq!(
        owner.lock().acknowledgement().saved.unwrap().text.as_ref(),
        "100"
    );
}

#[test]
fn failure_retains_saved_generation_and_identical_latest_snapshot_retries() {
    let owner = Mutex::new(Writer::new());
    let (result_tx, result_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    owner
        .lock()
        .submit("layout.json".into(), "old".into(), move |snapshot| {
            result_tx.send(snapshot.text.to_string()).unwrap();
            release_rx.recv().unwrap()
        })
        .unwrap();
    assert_eq!(
        result_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "old"
    );
    release_tx.send(Ok(())).unwrap();
    wait_idle(&owner);
    let mut layout = Layout::default();
    layout.acknowledge(owner.lock().acknowledgement());
    assert_eq!(layout.saved.as_deref(), Some("old"));
    owner
        .lock()
        .submit("layout.json".into(), "new".into(), |_| unreachable!())
        .unwrap();
    assert_eq!(
        result_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "new"
    );
    release_tx
        .send(Err("UI layout: replacement denied".into()))
        .unwrap();
    wait_idle(&owner);
    layout.acknowledge(owner.lock().acknowledgement());
    assert_eq!(layout.saved.as_deref(), Some("old"));
    assert!(layout.notice.contains("replacement denied"));
    owner
        .lock()
        .submit("layout.json".into(), "new".into(), |_| unreachable!())
        .unwrap();
    assert_eq!(
        result_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "new"
    );
    release_tx.send(Ok(())).unwrap();
    wait_idle(&owner);
    layout.acknowledge(owner.lock().acknowledgement());
    assert_eq!(layout.saved.as_deref(), Some("new"));
    assert_eq!(layout.saved_generation, 3);
    assert!(shutdown_owner(&owner, Duration::from_secs(5)));
}

#[cfg(windows)]
#[test]
fn real_atomic_failure_preserves_bytes_and_success_ack_then_retries() {
    let root = std::env::temp_dir().join(format!("jc3vr-layout-worker-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("layout.json");
    fs::write(&path, "old").unwrap();
    let owner = Mutex::new(Writer::new());
    let held = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    owner
        .lock()
        .submit(path.clone(), "new".into(), replace)
        .unwrap();
    wait_idle(&owner);
    assert!(owner.lock().acknowledgement().saved.is_none());
    assert!(owner.lock().acknowledgement().error.is_some());
    drop(held);
    assert_eq!(fs::read_to_string(&path).unwrap(), "old");
    owner
        .lock()
        .submit(path.clone(), "new".into(), replace)
        .unwrap();
    wait_idle(&owner);
    assert_eq!(
        owner.lock().acknowledgement().saved.unwrap().text.as_ref(),
        "new"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    assert!(shutdown_owner(&owner, Duration::from_secs(5)));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn restoring_an_older_value_supersedes_pending_newer_value() {
    let owner = Mutex::new(Writer::new());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    owner
        .lock()
        .submit("layout.json".into(), "old".into(), move |snapshot| {
            entered_tx.send(snapshot.text.to_string()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "old"
    );
    owner
        .lock()
        .submit("layout.json".into(), "new".into(), |_| unreachable!())
        .unwrap();
    owner
        .lock()
        .submit("layout.json".into(), "old".into(), |_| unreachable!())
        .unwrap();
    release_tx.send(()).unwrap();
    assert_eq!(
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "old"
    );
    release_tx.send(()).unwrap();
    assert!(shutdown_owner(&owner, Duration::from_secs(5)));
    assert_eq!(owner.lock().acknowledgement().saved.unwrap().generation, 3);
}

#[test]
fn shutdown_joins_panicked_owner_once_but_never_claims_stranded_work_drained() {
    for pending in [false, true] {
        let shared = Arc::new(Shared::default());
        if pending {
            shared.queue.lock().pending = Some(Snapshot {
                generation: 1,
                path: "layout.json".into(),
                text: "pending".into(),
            });
        }
        let owner = Mutex::new(Writer {
            shared: Some(shared),
            handle: Some(std::thread::spawn(|| panic!("injected writer termination"))),
            closed: false,
        });
        assert_eq!(shutdown_owner(&owner, Duration::from_secs(5)), !pending);
        assert!(owner.lock().handle.is_none());
        assert!(
            owner
                .lock()
                .acknowledgement()
                .error
                .unwrap()
                .contains("thread panicked")
        );
        assert_eq!(shutdown_owner(&owner, Duration::ZERO), !pending);
    }
}

fn wait_idle(owner: &Mutex<Writer>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let shared = Arc::clone(owner.lock().shared.as_ref().unwrap());
        let queue = shared.queue.lock();
        if !queue.in_flight && queue.pending.is_none() {
            return;
        }
        drop(queue);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
