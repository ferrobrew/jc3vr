use std::sync::mpsc;

use tracking_format::input::{CapturedInput, DiscontinuityReason, InputRecord};

use super::*;
use crate::{
    controllers::frame::{Button, HandFrame},
    recorder::tracking::format::{
        ControllerSample, Frame, Header, RecenterEvent, Recording, SessionSample, StartMarker,
    },
    vr::{RecenterCause, RecenterKind},
};

fn state() -> State {
    State {
        pending: None,
        stop: false,
        next_run: 0,
        phase: PhaseState::Idle,
        last: None,
        orphans: Vec::new(),
        shutting_down: false,
        stop_owned: None,
    }
}

fn request(id: u32) -> Request {
    Request {
        run: RunId(id),
        path: "fixture.tracking".into(),
        options: ReplayOptions::default(),
    }
}

pub(super) fn fixture() -> Arc<Loaded> {
    let frame = |time_ns| Frame {
        time_ns,
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
    let header = Header {
        build: "fixture".into(),
        recorded_at: "fixture".into(),
        action_schema: 4,
        start: StartMarker {
            transform: [0.0; 16],
            bookmark: None,
        },
        dominant_hand: controller_frame::Hand::Right,
        world_scale: 1.0,
        time_scale: 1.0,
    };
    let input = [100, 110, 130]
        .map(|capture_time_ns| CapturedInput {
            capture_time_ns,
            frame_sequence: Some(capture_time_ns),
            record: InputRecord::Discontinuity {
                reason: DiscontinuityReason::Unavailable,
                discarded: 0,
            },
        })
        .to_vec();
    let input_replay = tracking_format::input::replay::PreparedInput::new(&input);
    Arc::new(Loaded {
        path: "fixture.tracking".into(),
        recording: Recording {
            header,
            frames: vec![frame(100), frame(110)],
            input,
            version: 2,
            truncated: false,
            skipped_records: 0,
        },
        times: vec![100, 110],
        semantic: true,
        input_available: true,
        input_replay,
        warnings: Vec::new(),
    })
}

#[test]
fn consumed_start_transform_is_validated_without_rejecting_unused_markers() {
    let loaded = fixture();
    let valid = glam::Mat4::IDENTITY.to_cols_array();
    assert!(
        resolve_start(&loaded, StartMode::InPlace, || panic!("unused bookmark"))
            .unwrap()
            .is_none()
    );
    assert!(resolve_start(&loaded, StartMode::Marker, || panic!("unused bookmark")).is_err());
    assert_eq!(
        resolve_start(&loaded, StartMode::Bookmark, || Some(valid)).unwrap(),
        Some(valid)
    );
    assert!(resolve_start(&loaded, StartMode::Bookmark, || None).is_err());
    let mut invalid = valid;
    invalid[0] = f32::NAN;
    assert!(resolve_start(&loaded, StartMode::Bookmark, || Some(invalid)).is_err());
}

#[test]
fn runtime_replay_nonzero_origin_tail_and_repeated_start_stop() {
    let _serial = history::TEST_LOCK.lock();
    let now = Instant::now();
    let loaded = fixture();
    let mut state = state();
    for (index, pacing) in [Pacing::RealTime, Pacing::FrameLocked]
        .into_iter()
        .enumerate()
    {
        let run = Run {
            id: RunId(index as u32),
            options: ReplayOptions {
                start: StartMode::InPlace,
                pacing,
            },
        };
        state.play(run, Arc::clone(&loaded));
        assert!(state.stage_frame(now).unwrap().is_empty());
        assert_eq!(
            take_frames().unwrap()[0].display_time_ns,
            DISPLAY_TIME_ORIGIN_NS + 100
        );
        assert!(take_frames().is_none());
        assert_eq!(history::replayed_for_test().1, loaded.recording.input[..1]);
        history::take_gameplay();
        state.stage_frame(now + Duration::from_nanos(10)).unwrap();
        assert_eq!(take_frames().unwrap().len(), 1);
        state.advance();
        assert!(matches!(state.phase, PhaseState::Playing { .. }));
        assert_eq!(history::replayed_for_test().1, loaded.recording.input[1..2]);
        history::take_gameplay();
        state.stage_frame(now + Duration::from_nanos(30)).unwrap();
        assert!(take_frames().is_none());
        assert_eq!(history::replayed_for_test().1, loaded.recording.input[2..]);
        history::take_gameplay();
        state.advance();
        assert!(matches!(state.phase, PhaseState::Idle));
        assert!(matches!(
            state.last.as_ref().unwrap().1,
            ReplayOutcome::Completed { frames: 2, .. }
        ));
        assert_eq!(history::replayed_for_test(), (false, Vec::new()));
        state.play(run, Arc::clone(&loaded));
        state.stage_frame(now).unwrap();
        assert_eq!(history::replayed_for_test().1.len(), 1);
        state.finish(ReplayOutcome::Stopped);
        assert!(matches!(state.phase, PhaseState::Idle));
        assert_eq!(history::replayed_for_test(), (false, Vec::new()));
        assert!(take_frames().is_none());
    }
    history::take_capture();
}

#[test]
fn blocked_loaders_retain_capacity_and_recover_after_reaping() {
    let _serial = history::TEST_LOCK.lock();
    let mut state = state();
    let mut releases = Vec::new();
    for id in 1..=2 {
        state.admit_loader().unwrap();
        let (send, receive) = mpsc::channel();
        releases.push(send);
        let handle = std::thread::spawn(move || {
            receive.recv().unwrap();
            Err(ReplayError::Disabled)
        });
        state.phase = PhaseState::Loading {
            request: request(id),
            handle,
        };
        state.finish(ReplayOutcome::Stopped);
    }
    assert!(matches!(
        state.admit_loader(),
        Err(ReplayError::LoaderCapacity)
    ));
    assert_eq!(state.orphans.len(), 2);
    releases.remove(0).send(()).unwrap();
    let (done, wait) = mpsc::channel();
    let handle = state.orphans.remove(0);
    assert!(handle.join().unwrap().is_err());
    state.orphans.push(std::thread::spawn(move || {
        done.send(()).unwrap();
        Err(ReplayError::Disabled)
    }));
    wait.recv().unwrap();
    // A finished handle is kept until the production admission reaps and joins it.
    let deadline = Instant::now() + Duration::from_secs(2);
    while !state.orphans.last().unwrap().is_finished() {
        assert!(Instant::now() < deadline, "loader completion deadline");
        std::thread::yield_now();
    }
    state.admit_loader().unwrap();
    assert_eq!(state.orphans.len(), 1);
    releases.remove(0).send(()).unwrap();
    assert!(state.orphans.pop().unwrap().join().unwrap().is_err());
}

#[test]
fn scoped_stop_preserves_current_pending_and_loading_replacements() {
    let _serial = history::TEST_LOCK.lock();
    let mut state = state();
    for pending in [false, true] {
        state.play(
            Run {
                id: RunId(2),
                options: ReplayOptions::default(),
            },
            fixture(),
        );
        if pending {
            state.pending = Some(request(3));
        }
        state.cancel_owned(RunId(1));
        assert_eq!(state.run_id(), Some(RunId(2)));
        assert_eq!(
            state.pending.as_ref().map(|r| r.run),
            pending.then_some(RunId(3))
        );
        state.cancel_owned(RunId(2));
        assert!(matches!(state.phase, PhaseState::Idle));
        assert_eq!(
            state.pending.as_ref().map(|r| r.run),
            pending.then_some(RunId(3))
        );
        if pending {
            state.cancel_owned(RunId(3));
            assert!(state.pending.is_none());
        }
    }
    state.play(
        Run {
            id: RunId(6),
            options: ReplayOptions::default(),
        },
        fixture(),
    );
    state.pending = Some(request(7));
    state.request_stop_owned(RunId(6));
    state.request_stop_owned(RunId(7));
    state.request_stop_owned(RunId(1));
    assert!(state.pending.is_none());
    assert_eq!(state.stop_owned, Some(RunId(6)));
    let stopped = state.stop_owned.take().unwrap();
    state.cancel_owned(stopped);
    let (release, wait) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        wait.recv().unwrap();
        Err(ReplayError::Disabled)
    });
    state.phase = PhaseState::Loading {
        request: request(4),
        handle,
    };
    state.pending = Some(request(5));
    state.cancel_owned(RunId(1));
    assert_eq!(state.run_id(), Some(RunId(4)));
    assert_eq!(state.pending.as_ref().unwrap().run, RunId(5));
    release.send(()).unwrap();
    state.finish(ReplayOutcome::Stopped);
    assert!(state.orphans.pop().unwrap().join().unwrap().is_err());
}

#[test]
fn shutdown_retains_active_and_orphan_loaders_until_joined() {
    let _serial = history::TEST_LOCK.lock();
    let owner = Mutex::new(state());
    let mut releases = Vec::new();
    for id in 1..=2 {
        let (send, receive) = mpsc::channel();
        releases.push(send);
        let handle = std::thread::spawn(move || {
            receive.recv().unwrap();
            Err(ReplayError::Disabled)
        });
        let mut state = owner.lock();
        state.phase = PhaseState::Loading {
            request: request(id),
            handle,
        };
        if id == 1 {
            state.finish(ReplayOutcome::Stopped);
        }
    }
    for _ in 0..2 {
        assert!(!shutdown_state(&owner, Duration::ZERO));
        assert_eq!(owner.lock().orphans.len(), 2);
    }
    assert!(matches!(
        owner.lock().admit_loader(),
        Err(ReplayError::ShuttingDown)
    ));
    for release in releases {
        release.send(()).unwrap();
    }
    assert!(shutdown_state(&owner, Duration::from_secs(2)));
    assert!(owner.lock().orphans.is_empty());
}

#[test]
fn equal_stamp_pose_and_input_backlogs_are_bounded_without_eviction() {
    let _serial = history::TEST_LOCK.lock();
    let mut loaded = fixture();
    let data = Arc::get_mut(&mut loaded).unwrap();
    data.recording.frames = vec![data.recording.frames[0].clone(); 4];
    data.times = vec![100; 4];
    for (index, frame) in data.recording.frames.iter_mut().enumerate() {
        let mut hand = HandFrame::default();
        hand.buttons.set(Button::TriggerClick, index % 2 == 0);
        frame.controllers = Some(ControllerSample {
            focused: true,
            action_set: controller_frame::ActionSetKind::OnFoot,
            hands: [hand, HandFrame::default()],
        });
        frame.recenters.push(RecenterEvent {
            kind: RecenterKind::Full,
            cause: RecenterCause::Manual,
            turn: glam::Quat::from_rotation_y(index as f32 * 0.1),
        });
    }
    data.recording.input = (0..400)
        .map(|discarded| CapturedInput {
            capture_time_ns: 100,
            frame_sequence: None,
            record: InputRecord::Discontinuity {
                reason: DiscontinuityReason::Unavailable,
                discarded,
            },
        })
        .collect();
    data.input_replay = tracking_format::input::replay::PreparedInput::new(&data.recording.input);
    let mut state = state();
    state.play(
        Run {
            id: RunId(1),
            options: ReplayOptions::default(),
        },
        Arc::clone(&loaded),
    );
    let now = Instant::now();
    assert_eq!(
        state.stage_frame(now).unwrap(),
        loaded.recording.frames[0].recenters
    );
    let first = take_frames().unwrap();
    assert_eq!(first.len(), 1);
    assert!(
        first[0].controllers.as_ref().unwrap().hands[0]
            .buttons
            .contains(Button::TriggerClick)
    );
    assert_eq!(
        history::replayed_for_test().1,
        loaded.recording.input[..INPUT_BUDGET]
    );
    assert!(state.stage_frame(now).unwrap().is_empty());
    assert!(take_frames().is_none());
    let mut actual = history::replayed_for_test().1;
    history::take_gameplay();
    for _ in 0..3 {
        state.stage_frame(now).unwrap();
        assert!(take_frames().is_none());
        let chunk = history::replayed_for_test().1;
        assert!(chunk.len() <= INPUT_BUDGET);
        actual.extend(chunk);
        history::take_gameplay();
    }
    assert_eq!(actual, loaded.recording.input);
    for index in 1..4 {
        assert_eq!(
            state.stage_frame(now).unwrap(),
            loaded.recording.frames[index].recenters
        );
        let frames = take_frames().unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].controllers.as_ref().unwrap().hands[0]
                .buttons
                .contains(Button::TriggerClick),
            index % 2 == 0
        );
    }
    state.advance();
    assert!(matches!(state.phase, PhaseState::Idle));
}

#[test]
fn input_pacing_uses_capture_time_and_consumes_each_record_once() {
    let records = [10, 20, 30].map(|capture_time_ns| CapturedInput {
        capture_time_ns,
        frame_sequence: None,
        record: InputRecord::Discontinuity {
            reason: DiscontinuityReason::Unavailable,
            discarded: capture_time_ns,
        },
    });
    let prepared = tracking_format::input::replay::PreparedInput::new(&records);
    for deadlines in [vec![10, 19, 20, 30], vec![30]] {
        let mut cursor = InputReplay::default();
        let mut actual = Vec::new();
        for elapsed in deadlines {
            let range = cursor.advance(
                &prepared,
                input_deadline(Pacing::RealTime, elapsed, 0, None, 30),
            );
            actual.extend_from_slice(&records[range]);
        }
        assert_eq!(actual, records);
        assert!(cursor.finished(records.len()));
    }
    let mut cursor = InputReplay::default();
    for (frame, expected) in [(Some(10), 0..1), (Some(20), 1..2), (None, 2..3)] {
        assert_eq!(
            cursor.advance(
                &prepared,
                input_deadline(Pacing::FrameLocked, 9999, 0, frame, 30)
            ),
            expected
        );
    }
}
