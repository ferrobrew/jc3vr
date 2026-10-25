//! The tracking recorder's owned snapshots and non-reaping query.

use super::*;
use crate::recorder::tracking::writer::{Completion, ControlledCompletion};

#[test]
fn local_snapshot_preserves_stopped_fields_and_completed_writer_ownership() {
    let _serial = history::TEST_LOCK.lock();
    let (mut state, completion) = completed_state();
    let expected = state.status.clone();
    for _ in 0..2 {
        let snapshot = state.snapshot();
        assert_eq!(snapshot.recording, is_recording());
        assert_eq!(snapshot.frames, FRAMES.load(Ordering::Relaxed));
        assert_eq!(snapshot.dropped, DROPPED.load(Ordering::Relaxed));
        assert_stopped_fields(&snapshot, &expected);
        assert_stopped_fields(&state.status, &expected);
        assert_eq!(state.finishing.len(), 1);
        assert!(completion.lock().is_some());
        assert!(!state.shutting_down);
    }
    state.reap();
    assert!(state.finishing.is_empty());
    assert!(completion.lock().is_none());
    assert_eq!(state.status.bytes, 8192);
    assert_eq!(
        state.status.error.as_deref(),
        Some("tracking writer: sync: completed fixture")
    );
}

#[test]
fn local_snapshot_reads_active_progress_without_stopping_the_writer() {
    let _serial = history::TEST_LOCK.lock();
    let (writer, completion) = Writer::controlled();
    let path = PathBuf::from("active-query.tracking");
    let started = Instant::now() - Duration::from_secs(2);
    let state = State {
        active: Some(Active {
            writer,
            path: path.clone(),
            started,
            hand_joints: true,
            recenters: Vec::new(),
        }),
        finishing: Vec::new(),
        shutting_down: false,
        status: TrackingStatus {
            path: Some(path.clone()),
            bytes: 4096,
            logical_bytes: 8192,
            duration: Duration::from_secs(1),
            ..TrackingStatus::default()
        },
    };
    let snapshot = state.snapshot();
    assert_eq!(snapshot.path, Some(path));
    assert_eq!(snapshot.bytes, 0);
    assert_eq!(snapshot.logical_bytes, 0);
    assert!(snapshot.duration >= Duration::from_secs(2));
    assert!(snapshot.duration <= started.elapsed());
    assert_eq!(state.status.bytes, 4096);
    assert_eq!(state.status.logical_bytes, 8192);
    assert_eq!(state.status.duration, Duration::from_secs(1));
    assert!(state.active.as_ref().unwrap().hand_joints);
    assert!(state.finishing.is_empty());
    assert!(completion.lock().is_none());
    assert!(!state.shutting_down);
}

#[test]
fn public_peek_preserves_completion_and_requests_while_status_still_reaps() {
    let _serial = history::TEST_LOCK.lock();
    let (state, completion) = completed_state();
    let expected = state.status.clone();
    let _restore = RestoreRecorder {
        state: Some(std::mem::replace(&mut *STATE.lock(), state)),
        request: REQUEST.swap(REQUEST_STOP, Ordering::Relaxed),
        recording: RECORDING.swap(false, Ordering::Relaxed),
        frames: FRAMES.swap(37, Ordering::Relaxed),
        dropped: DROPPED.swap(4, Ordering::Relaxed),
    };
    for _ in 0..2 {
        let snapshot = peek_status();
        assert!(!snapshot.recording);
        assert_eq!(snapshot.frames, 37);
        assert_eq!(snapshot.dropped, 4);
        assert_stopped_fields(&snapshot, &expected);
        let state = STATE.lock();
        assert_eq!(state.finishing.len(), 1);
        assert!(state.active.is_none());
        assert!(!state.shutting_down);
        assert_stopped_fields(&state.status, &expected);
        assert!(completion.lock().is_some());
        assert_eq!(REQUEST.load(Ordering::Relaxed), REQUEST_STOP);
        assert!(!is_recording());
    }
    let collected = status();
    assert_eq!(collected.bytes, 8192);
    assert_eq!(
        collected.error.as_deref(),
        Some("tracking writer: sync: completed fixture")
    );
    assert!(STATE.lock().finishing.is_empty());
    assert!(completion.lock().is_none());
    assert_eq!(REQUEST.load(Ordering::Relaxed), REQUEST_STOP);
    assert!(!is_recording());
}

fn completed_state() -> (State, ControlledCompletion) {
    let (writer, completion) = Writer::controlled();
    *completion.lock() = Some(Completion {
        bytes: 8192,
        error: Some("tracking writer: sync: completed fixture".to_owned()),
    });
    let path = PathBuf::from("completed-query.tracking");
    (
        State {
            active: None,
            finishing: vec![Finishing {
                writer,
                path: path.clone(),
            }],
            shutting_down: false,
            status: TrackingStatus {
                recording: true,
                path: Some(path),
                frames: 2,
                dropped: 1,
                bytes: 4096,
                logical_bytes: 6144,
                duration: Duration::from_secs(3),
                error: Some("tracking writer: previous fixture".to_owned()),
            },
        },
        completion,
    )
}

fn assert_stopped_fields(actual: &TrackingStatus, expected: &TrackingStatus) {
    assert_eq!(actual.path, expected.path);
    assert_eq!(actual.bytes, expected.bytes);
    assert_eq!(actual.logical_bytes, expected.logical_bytes);
    assert_eq!(actual.duration, expected.duration);
    assert_eq!(actual.error, expected.error);
}

struct RestoreRecorder {
    state: Option<State>,
    request: u8,
    recording: bool,
    frames: u32,
    dropped: u32,
}

impl Drop for RestoreRecorder {
    fn drop(&mut self) {
        *STATE.lock() = self.state.take().unwrap();
        REQUEST.store(self.request, Ordering::Relaxed);
        RECORDING.store(self.recording, Ordering::Relaxed);
        FRAMES.store(self.frames, Ordering::Relaxed);
        DROPPED.store(self.dropped, Ordering::Relaxed);
    }
}
