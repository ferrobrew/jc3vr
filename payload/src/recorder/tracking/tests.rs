//! The tracking recorder's writer ownership, capture draining, and capture batch wire.

use tracking_format::input::DiscontinuityReason;

use super::*;
use crate::{controllers::frame::Hand, vr::stage::StageInfo};

pub(super) fn assert_active_failure_transition(completion: writer::Completion) {
    let _serial = history::TEST_LOCK.lock();
    let expected_bytes = completion.bytes;
    let expected_error = completion.error.clone();
    let path = PathBuf::from("failed-active.tracking");
    let (writer, completed) = Writer::controlled();
    *completed.lock() = Some(completion);
    let previous = {
        let mut state = STATE.lock();
        std::mem::replace(
            &mut *state,
            State {
                active: Some(Active {
                    writer,
                    path: path.clone(),
                    started: Instant::now(),
                    hand_joints: false,
                }),
                status: TrackingStatus {
                    path: Some(path.clone()),
                    ..TrackingStatus::default()
                },
                ..State::idle()
            },
        )
    };
    let previous_recording = RECORDING.swap(true, Ordering::Relaxed);
    let previous_request = REQUEST.swap(REQUEST_NONE, Ordering::Relaxed);
    frame_tick();
    let observed = status();
    let (active, finishing) = {
        let mut state = STATE.lock();
        let active = state.active.is_some();
        let finishing = state.finishing.len();
        *state = previous;
        (active, finishing)
    };
    RECORDING.store(previous_recording, Ordering::Relaxed);
    REQUEST.store(previous_request, Ordering::Relaxed);
    assert!(!active);
    assert_eq!(finishing, 0);
    assert!(!observed.recording);
    assert_eq!(observed.path, Some(path));
    assert_eq!(observed.bytes, expected_bytes);
    assert_eq!(observed.error, expected_error);
}

fn local_state() -> State {
    State::idle()
}

fn activate(state: &mut State, writer: Writer, name: &str) {
    state.can_start().unwrap();
    let path = PathBuf::from(name);
    state.status = TrackingStatus {
        path: Some(path.clone()),
        ..TrackingStatus::default()
    };
    state.active = Some(Active {
        writer,
        path,
        started: Instant::now(),
        hand_joints: false,
    });
}

#[test]
fn rapid_stops_retain_all_pending_owners_and_refuse_a_saturated_start() {
    let mut state = local_state();
    let (a, complete_a) = Writer::controlled();
    let (b, complete_b) = Writer::controlled();
    let (c, complete_c) = Writer::controlled();
    activate(&mut state, a, "a.tracking");
    state.stop_active();
    activate(&mut state, c, "c.tracking");
    state.stop_active();
    activate(&mut state, b, "b.tracking");
    state.stop_active();
    assert_eq!(state.finishing.len(), MAX_WRITERS);
    assert!(matches!(state.can_start(), Err(StartError::WritersBusy)));
    *complete_b.lock() = Some(writer::Completion {
        bytes: 22,
        error: Some("tracking writer: sync: fixture".into()),
    });
    *complete_c.lock() = Some(writer::Completion {
        bytes: 33,
        error: None,
    });
    state.reap();
    assert_eq!(state.finishing.len(), 1);
    assert_eq!(state.status.path, Some(PathBuf::from("b.tracking")));
    assert_eq!(state.status.bytes, 22);
    assert!(state.status.error.as_ref().unwrap().contains("sync"));
    assert!(!state.shutdown());
    assert_eq!(state.finishing[0].path, PathBuf::from("a.tracking"));
    assert!(matches!(state.can_start(), Err(StartError::ShuttingDown)));
    *complete_a.lock() = Some(writer::Completion {
        bytes: 11,
        error: Some("tracking writer: record write: older".into()),
    });
    assert!(state.shutdown());
    assert!(state.finishing.is_empty());
    assert_eq!(state.status.bytes, 22);
    assert!(state.status.error.as_ref().unwrap().contains("sync"));
}

#[test]
fn completion_frees_capacity_and_updates_the_matching_stopped_recording() {
    let mut state = local_state();
    let (a, complete_a) = Writer::controlled();
    activate(&mut state, a, "a.tracking");
    state.stop_active();
    state.reap();
    assert_eq!(state.status.bytes, 0);
    *complete_a.lock() = Some(writer::Completion {
        bytes: 100,
        error: None,
    });
    state.can_start().unwrap();
    assert!(state.finishing.is_empty());
    assert_eq!(state.status.bytes, 100);
    assert!(state.status.error.is_none());
    assert!(state.shutdown());
}

#[test]
fn actual_idle_capture_and_start_boundary_drain_pending_publications() {
    let _serial = history::TEST_LOCK.lock();
    history::take_capture();
    history::interrupt(DiscontinuityReason::Unavailable);
    capture(CaptureSample {
        display_time_ns: 90,
        should_render: true,
        head_tracked: false,
        eyes: None,
    });
    assert!(history::take_capture().is_empty());
    history::interrupt(DiscontinuityReason::Unavailable);
    discard_before_start();
    assert!(history::take_capture().is_empty());
}

#[test]
fn snapshot_matching_requires_publication_identity_even_with_equal_display_times() {
    let now = Instant::now();
    let frame = controller_frame::ControllerFrame {
        sequence: 11,
        display_time_ns: 90,
        published_at: now,
        focused: true,
        action_set: controller_frame::ActionSetKind::OnFoot,
        hands: Default::default(),
    };
    let set = |set| semantic_frame::SetState {
        set,
        actions: Vec::new(),
    };
    let semantic = semantic_frame::SemanticFrame {
        published_at: now,
        action_set: frame.action_set,
        global: set(crate::vr::input::semantic::SemanticSet::Global),
        mode: set(crate::vr::input::semantic::SemanticSet::OnFoot),
    };
    let input = [CapturedInput {
        capture_time_ns: 1,
        frame_sequence: Some(12),
        record: InputRecord::Discontinuity {
            reason: DiscontinuityReason::Unavailable,
            discarded: 0,
        },
    }];
    let (raw, actions) = matching_snapshots(Some(&frame), Some(&semantic), &input);
    assert!(raw.is_none());
    assert!(actions.is_none());
    let current = controller_frame::ControllerFrame {
        sequence: 12,
        ..frame
    };
    assert_eq!(current.display_time_ns, 90);
    let (raw, actions) = matching_snapshots(Some(&current), Some(&semantic), &input);
    assert_eq!(raw.unwrap().sequence, 12);
    assert!(actions.is_some());
    let stale = semantic_frame::SemanticFrame {
        published_at: now - Duration::from_nanos(1),
        ..semantic
    };
    let (raw, actions) = matching_snapshots(Some(&current), Some(&stale), &input);
    assert!(raw.is_some());
    assert!(actions.is_none());
}

#[test]
fn capture_wire_preserves_exact_publication_time_and_missing_associations() {
    let started = Instant::now();
    let record = InputRecord::Discontinuity {
        reason: DiscontinuityReason::Unavailable,
        discarded: 0,
    };
    let publications = vec![
        Publication {
            observed_at: started - Duration::from_nanos(1),
            frame_sequence: Some(1),
            record: record.clone(),
        },
        Publication {
            observed_at: started + Duration::from_nanos(7),
            frame_sequence: Some(12),
            record: record.clone(),
        },
        Publication {
            observed_at: started + Duration::from_nanos(9),
            frame_sequence: None,
            record,
        },
    ];
    let captured = capture_publications(&publications, started);
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].capture_time_ns, 7);
    assert_eq!(captured[0].frame_sequence, Some(12));
    assert_eq!(captured[1].capture_time_ns, 9);
    assert_eq!(captured[1].frame_sequence, None);
    let frame = Frame {
        time_ns: 15,
        display_time_ns: 90,
        session: SessionSample {
            state: None,
            should_render: true,
            head_tracked: false,
        },
        head: None,
        stage: StageInfo::default(),
        controllers: None,
        semantic: None,
        recenters: Vec::new(),
    };
    let batch = encode_capture(&frame, &captured, &[]).unwrap();
    let header = Header {
        build: "fixture".into(),
        recorded_at: "fixture".into(),
        action_schema: 4,
        start: StartMarker {
            transform: [0.0; 16],
            bookmark: None,
        },
        dominant_hand: Hand::Right,
        world_scale: 1.0,
        time_scale: 1.0,
    };
    let mut bytes = format::encode_input_preamble(&header);
    bytes.extend(batch.bytes);
    let decoded = format::decode(&bytes).unwrap();
    assert_eq!(decoded.version, tracking_format::VERSION);
    assert_eq!(decoded.input, captured);
    assert_eq!(decoded.frames, vec![frame]);
}
