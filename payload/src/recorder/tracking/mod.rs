//! The tracking recorder: writes what the OpenXR runtime gives the mod each frame (the eye views,
//! both controllers' poses and inputs, the semantic action states, the tracking validity, and the
//! session state) to a file in the session folder, with a start marker naming where the player
//! stood, so that a replay ([`crate::testing::replay`]) can drive the game with it again without
//! the headset.
//!
//! The recorder samples at the point the frame loop has published everything a frame reads: the
//! end of `VrState::begin_frame`, on the game thread under the runtime lock, after the eye views
//! are located and the controllers are sampled. It records the published state, not the runtime's
//! raw answers, so the replay can publish it in their place and every consumer sees the same thing
//! it saw live. [`capture`] encodes the frame there and hands the bytes to a writer thread
//! ([`writer`]), and takes only leaf locks (the published slots and its own state).
//!
//! A recording starts and stops on request ([`request_start`], [`request_stop`], [`toggle`]) from
//! any thread: the debug UI, the director's key bindings, or the testing request file. The request
//! is serviced on the game thread at the top of the next frame ([`frame_tick`]), which is where the
//! player's transform can be read for the start marker. Nothing is recorded while a replay drives
//! the game. The file format is [`format`]; see `docs/mod/testing/recorder.md`.

pub(crate) mod format;
mod writer;

use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use tracking_format::input::{CapturedInput, InputRecord};

use crate::{
    config::Config,
    controllers::frame as controller_frame,
    recorder::tracking::{
        format::{
            ControllerSample, EyeSample, Frame, HeadSample, Header, RecenterEvent, SemanticSample,
            SessionSample, StartMarker,
        },
        writer::{CaptureBatch, SendOutcome, Writer},
    },
    vr::{
        input::semantic::{
            frame as semantic_frame,
            history::{self, Publication},
        },
        stage,
    },
};

/// What the frame loop knows about a frame's views when it hands it to [`capture`].
#[derive(Debug, Copy, Clone)]
pub(crate) struct CaptureSample {
    /// The runtime's predicted display time.
    pub(crate) display_time_ns: i64,
    /// Whether the runtime asked for the frame to be rendered.
    pub(crate) should_render: bool,
    /// Whether the view pose was valid.
    pub(crate) head_tracked: bool,
    /// Both eyes in the cockpit frame, when the views were located.
    pub(crate) eyes: Option<[EyeSample; 2]>,
}

/// The tracking recorder's state for the debug UI.
#[derive(Debug, Clone, Default)]
pub(crate) struct TrackingStatus {
    pub(crate) recording: bool,
    /// The file of the current or last recording.
    pub(crate) path: Option<PathBuf>,
    pub(crate) frames: u32,
    /// Frames dropped because the writer fell behind.
    pub(crate) dropped: u32,
    /// Stored bytes accepted by the buffered sink, including the preamble. A flush error can
    /// prevent durability. Version 3 stores chunks; version 2 stores original serialized records.
    pub(crate) bytes: u64,
    /// Preamble and original record bytes for complete accepted batches, before compression.
    pub(crate) logical_bytes: u64,
    /// How long the current or last recording ran.
    pub(crate) duration: Duration,
    /// Why the last recording did not start or stopped early.
    pub(crate) error: Option<String>,
}

/// Why a recording did not start.
#[derive(Debug)]
pub(crate) enum StartError {
    /// The test tools are switched off.
    Disabled,
    /// A replay is driving the game, so there is nothing live to record.
    Replaying,
    /// There is no player character to mark the start at.
    NoPlayer,
    /// The session folder is unavailable.
    NoSessionFolder,
    /// The session folder's `tracking` directory could not be created.
    Folder(std::io::Error),
    Writer(writer::WriterError),
    /// The bounded owner set is full of writers that have not exited.
    WritersBusy,
    ShuttingDown,
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "recorder: tracking: ")?;
        match self {
            StartError::Disabled => write!(f, "the test tools are switched off (testing.enabled)"),
            StartError::Replaying => write!(f, "a replay is running"),
            StartError::NoPlayer => write!(f, "there is no player character to mark the start at"),
            StartError::NoSessionFolder => write!(f, "the session folder is unavailable"),
            StartError::Folder(e) => {
                write!(f, "could not create the session's tracking folder: {e}")
            }
            StartError::Writer(e) => write!(f, "{e}"),
            StartError::WritersBusy => write!(f, "previous recording writers are still finishing"),
            StartError::ShuttingDown => write!(f, "the recorder is shutting down"),
        }
    }
}

impl std::error::Error for StartError {}

/// Ask for a recording to start at the top of the next frame. Any thread.
pub(crate) fn request_start() {
    REQUEST.store(REQUEST_START, Ordering::Relaxed);
}

/// Ask for the recording to stop at the top of the next frame. Any thread.
pub(crate) fn request_stop() {
    REQUEST.store(REQUEST_STOP, Ordering::Relaxed);
}

/// Ask for the recording to start when none runs, and to stop when one does. Any thread.
pub(crate) fn toggle() {
    if is_recording() {
        request_stop();
    } else {
        request_start();
    }
}

/// Whether a recording is running.
pub(crate) fn is_recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

/// The recorder's state.
pub(crate) fn status() -> TrackingStatus {
    let mut state = STATE.lock();
    state.reap();
    let mut status = state.status.clone();
    status.recording = is_recording();
    status.frames = FRAMES.load(Ordering::Relaxed);
    status.dropped = DROPPED.load(Ordering::Relaxed);
    if let Some(active) = &state.active {
        status.bytes = active.writer.bytes_written();
        status.logical_bytes = active.writer.logical_bytes_written();
        status.duration = active.started.elapsed();
    }
    status
}

/// Service a start or stop request. Game thread, once per frame, outside the VR runtime lock.
pub(crate) fn frame_tick() {
    STATE.lock().reap();
    match REQUEST.swap(REQUEST_NONE, Ordering::Relaxed) {
        REQUEST_START => start(),
        REQUEST_STOP => stop(),
        _ => {}
    }
    // A writer that failed on its own is noticed here even on a frame no sample reaches it.
    let failed = STATE.lock().active.as_ref().and_then(|active| {
        active.writer.error().or_else(|| {
            active
                .writer
                .is_finished()
                .then(|| "tracking writer: the writer thread exited while recording".to_owned())
        })
    });
    if let Some(error) = failed {
        fail(&error);
    }
}

/// Record one frame. Called by the frame loop on the game thread under the runtime lock, after
/// the views are located and the controllers are sampled; it takes only leaf locks. A no-op
/// unless recording.
pub(crate) fn capture(sample: CaptureSample) {
    let publications = history::take_capture();
    if !is_recording() {
        return;
    }
    let captured_at = Instant::now();
    let controllers = controller_frame::latest();
    let semantic = semantic_frame::latest();
    let session_state = crate::vr::published_status().session_state;
    let stage = stage::stage();
    let mut state = STATE.lock();
    let Some(active) = state.active.as_mut() else {
        return;
    };
    let input = capture_publications(publications, active.started);
    let (controllers, semantic) = matching_snapshots(controllers, semantic, &input);
    let frame = Frame {
        time_ns: nanos_since(captured_at, active.started),
        display_time_ns: sample.display_time_ns,
        session: SessionSample {
            state: session_state,
            should_render: sample.should_render,
            head_tracked: sample.head_tracked,
        },
        head: sample.eyes.map(|eyes| HeadSample { eyes }),
        stage,
        controllers: controllers.map(|controller| {
            let mut hands = controller.hands.clone();
            if !active.hand_joints {
                for hand in &mut hands {
                    hand.joints = None;
                }
            }
            ControllerSample {
                focused: controller.focused,
                action_set: controller.action_set,
                hands,
            }
        }),
        semantic: semantic.map(|semantic| SemanticSample {
            action_set: semantic.action_set,
            global: semantic.global,
            mode: semantic.mode,
        }),
        recenters: std::mem::take(&mut active.recenters),
    };
    let batch = match encode_capture(&frame, &input) {
        Ok(batch) => batch,
        Err(error) => {
            let error = error.to_string();
            drop(state);
            fail(&error);
            return;
        }
    };
    match active.writer.send(batch) {
        SendOutcome::Queued => {
            FRAMES.fetch_add(1, Ordering::Relaxed);
        }
        SendOutcome::Full => {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
        SendOutcome::Failed(error) => {
            drop(state);
            fail(&error);
        }
    }
}

fn matching_snapshots(
    controllers: Option<controller_frame::ControllerFrame>,
    semantic: Option<semantic_frame::SemanticFrame>,
    input: &[CapturedInput],
) -> (
    Option<controller_frame::ControllerFrame>,
    Option<semantic_frame::SemanticFrame>,
) {
    let controllers = controllers.filter(|frame| {
        input
            .iter()
            .any(|entry| entry.frame_sequence == Some(frame.sequence))
    });
    let semantic = semantic.filter(|semantic| {
        controllers
            .as_ref()
            .is_some_and(|frame| frame.published_at == semantic.published_at)
    });
    (controllers, semantic)
}

fn capture_publications(publications: Vec<Publication>, started: Instant) -> Vec<CapturedInput> {
    publications
        .into_iter()
        .filter(|entry| entry.observed_at >= started)
        .map(|entry| CapturedInput {
            capture_time_ns: nanos_since(entry.observed_at, started),
            frame_sequence: entry.frame_sequence,
            record: entry.record,
        })
        .collect()
}

fn nanos_since(at: Instant, origin: Instant) -> u64 {
    u64::try_from(at.saturating_duration_since(origin).as_nanos()).unwrap_or(u64::MAX)
}

fn encode_capture(
    frame: &Frame,
    input: &[CapturedInput],
) -> Result<CaptureBatch, format::DecodeError> {
    let mut bytes = Vec::new();
    let mut input_records = 0u64;
    for entry in input {
        input_records = input_records.saturating_add(match &entry.record {
            InputRecord::Group(_) | InputRecord::Policy(_) => 1,
            InputRecord::Discontinuity { discarded, .. } => *discarded,
        });
        let encoded = tracking_format::encode_input(entry)?;
        if bytes.len().saturating_add(encoded.len()) > writer::MAX_BATCH_BYTES {
            return Err(format::DecodeError {
                offset: 0,
                kind: tracking_format::DecodeErrorKind::InvalidValue {
                    field: "capture batch byte bound",
                    value: bytes.len().saturating_add(encoded.len()) as u64,
                },
            });
        }
        bytes.extend(encoded);
    }
    let encoded_frame = format::encode_frame(frame);
    if bytes.len().saturating_add(encoded_frame.len()) > writer::MAX_BATCH_BYTES {
        return Err(format::DecodeError {
            offset: 0,
            kind: tracking_format::DecodeErrorKind::InvalidValue {
                field: "capture batch byte bound",
                value: bytes.len().saturating_add(encoded_frame.len()) as u64,
            },
        });
    }
    bytes.extend(encoded_frame);
    Ok(CaptureBatch {
        bytes,
        capture_time_ns: frame.time_ns,
        input_records,
    })
}

/// Note a recenter of the cockpit baseline, recorded on the next frame. Game thread.
pub(crate) fn note_recenter(event: RecenterEvent) {
    if !is_recording() {
        return;
    }
    if let Some(active) = STATE.lock().active.as_mut() {
        active.recenters.push(event);
    }
}

/// Close every writer without waiting on disk. The eject caller must pin the payload when any
/// retained owner is still running. I/O failure after termination does not prevent unmapping.
#[must_use = "a writer that did not finish means the payload must stay mapped"]
pub(crate) fn shutdown() -> bool {
    let mut state = STATE.lock();
    REQUEST.store(REQUEST_NONE, Ordering::Relaxed);
    RECORDING.store(false, Ordering::Relaxed);
    state.shutdown()
}

/// Includes the active writer. Saturation refuses a start rather than discarding an owner.
const MAX_WRITERS: usize = 2;

/// How close to a director bookmark the player must stand for the start marker to name it.
const BOOKMARK_RADIUS_M: f32 = 2.0;

const REQUEST_NONE: u8 = 0;
const REQUEST_START: u8 = 1;
const REQUEST_STOP: u8 = 2;

static REQUEST: AtomicU8 = AtomicU8::new(REQUEST_NONE);
static RECORDING: AtomicBool = AtomicBool::new(false);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static DROPPED: AtomicU32 = AtomicU32::new(0);

struct State {
    active: Option<Active>,
    finishing: Vec<Finishing>,
    shutting_down: bool,
    status: TrackingStatus,
}

struct Finishing {
    writer: Writer,
    path: PathBuf,
}

struct Active {
    writer: Writer,
    path: PathBuf,
    started: Instant,
    hand_joints: bool,
    /// Recenters since the last recorded frame.
    recenters: Vec<RecenterEvent>,
}

impl State {
    fn shutdown(&mut self) -> bool {
        self.shutting_down = true;
        self.stop_active();
        self.reap();
        self.finishing.is_empty()
    }

    fn can_start(&mut self) -> Result<(), StartError> {
        self.reap();
        if self.shutting_down {
            return Err(StartError::ShuttingDown);
        }
        if self.finishing.len() + usize::from(self.active.is_some()) >= MAX_WRITERS {
            return Err(StartError::WritersBusy);
        }
        Ok(())
    }

    fn stop_active(&mut self) {
        let Some(mut active) = self.active.take() else {
            return;
        };
        self.status.bytes = active.writer.bytes_written();
        self.status.logical_bytes = active.writer.logical_bytes_written();
        self.status.duration = active.started.elapsed();
        active.writer.finish();
        self.finishing.push(Finishing {
            writer: active.writer,
            path: active.path,
        });
    }

    fn reap(&mut self) {
        let mut index = 0;
        while index < self.finishing.len() {
            let writer = &mut self.finishing[index];
            if let Some(completion) = writer.writer.poll_completion() {
                if let Some(error) = &completion.error {
                    tracing::error!(path = %writer.path.display(), "recorder: tracking: {error}");
                }
                if self.status.path.as_ref() == Some(&writer.path) {
                    self.status.bytes = completion.bytes;
                    self.status.logical_bytes = writer.writer.logical_bytes_written();
                    if completion.error.is_some() {
                        self.status.error = completion.error;
                    }
                }
                self.finishing.swap_remove(index);
            } else {
                if self.status.path.as_ref() == Some(&writer.path) {
                    self.status.bytes = writer.writer.bytes_written();
                    self.status.logical_bytes = writer.writer.logical_bytes_written();
                    if let Some(error) = writer.writer.error() {
                        self.status.error = Some(error);
                    }
                }
                index += 1;
            }
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    active: None,
    finishing: Vec::new(),
    shutting_down: false,
    status: TrackingStatus {
        recording: false,
        path: None,
        frames: 0,
        dropped: 0,
        bytes: 0,
        logical_bytes: 0,
        duration: Duration::ZERO,
        error: None,
    },
});

fn start() {
    if is_recording() {
        return;
    }
    match begin() {
        Ok(path) => tracing::info!("recorder: tracking: recording to {}", path.display()),
        Err(error) => {
            tracing::warn!("{error}");
            STATE.lock().status.error = Some(error.to_string());
        }
    }
}

fn begin() -> Result<PathBuf, StartError> {
    if !Config::lock_query(|c| c.testing.enabled) {
        return Err(StartError::Disabled);
    }
    if crate::testing::replay::is_active() {
        return Err(StartError::Replaying);
    }
    let transform = crate::director::teleport::player_transform().ok_or(StartError::NoPlayer)?;
    let marker = StartMarker {
        transform,
        bookmark: None,
    };
    let bookmark = crate::director::teleport::bookmarks()
        .into_iter()
        .map(|b| {
            let p = b.position();
            (b.name, (glam::Vec3::from(p) - marker.position()).length())
        })
        .filter(|(_, distance)| *distance <= BOOKMARK_RADIUS_M)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(name, _)| name);
    let (dominant_hand, world_scale, time_scale, hand_joints, compression) =
        Config::lock_query(|c| {
            (
                c.controllers.dominant_hand,
                c.vr.world_scale,
                c.director.time_scale,
                c.testing.record_hand_joints,
                c.testing.record_compression,
            )
        });
    let stamp = crate::session::stamp();
    let header = Header {
        build: crate::BUILD_STAMP.to_owned(),
        recorded_at: stamp.clone(),
        action_schema: crate::vr::input::semantic::SCHEMA_VERSION,
        start: StartMarker { bookmark, ..marker },
        dominant_hand,
        world_scale,
        time_scale,
    };
    let folder = crate::session::subdir("tracking")
        .ok_or(StartError::NoSessionFolder)?
        .map_err(StartError::Folder)?;
    let path = folder.join(format!("{stamp}.{}", format::EXTENSION));
    let mut state = STATE.lock();
    state.can_start()?;
    let recording_format = writer::RecordingFormat::from_compression_opt_in(compression);
    let writer = Writer::start(&path, &header, recording_format).map_err(StartError::Writer)?;
    discard_before_start();
    state.active = Some(Active {
        writer,
        path: path.clone(),
        started: Instant::now(),
        hand_joints,
        recenters: Vec::new(),
    });
    state.status = TrackingStatus {
        path: Some(path.clone()),
        ..TrackingStatus::default()
    };
    FRAMES.store(0, Ordering::Relaxed);
    DROPPED.store(0, Ordering::Relaxed);
    RECORDING.store(true, Ordering::Relaxed);
    if let Some(name) = &header.start.bookmark {
        tracing::info!("recorder: tracking: the start marker is at the bookmark {name:?}");
    }
    Ok(path)
}

fn discard_before_start() {
    history::begin_capture();
}

fn stop() {
    let mut state = STATE.lock();
    state.stop_active();
    state.reap();
    RECORDING.store(false, Ordering::Relaxed);
    drop(state);
    tracing::info!(
        frames = FRAMES.load(Ordering::Relaxed),
        dropped = DROPPED.load(Ordering::Relaxed),
        "recorder: tracking: stopped"
    );
}

fn fail(error: &str) {
    tracing::error!("recorder: tracking: {error}; stopping");
    STATE.lock().status.error = Some(error.to_owned());
    stop();
}

#[cfg(test)]
fn assert_active_failure_transition(completion: writer::Completion) {
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
                    recenters: Vec::new(),
                }),
                finishing: Vec::new(),
                shutting_down: false,
                status: TrackingStatus {
                    path: Some(path.clone()),
                    ..TrackingStatus::default()
                },
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

#[cfg(test)]
mod tests {
    use tracking_format::input::DiscontinuityReason;

    use super::*;
    use crate::{controllers::frame::Hand, vr::stage::StageInfo};

    fn local_state() -> State {
        State {
            active: None,
            finishing: Vec::new(),
            shutting_down: false,
            status: TrackingStatus::default(),
        }
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
            recenters: Vec::new(),
        });
    }

    #[test]
    fn rapid_stops_retain_all_pending_owners_and_refuse_a_saturated_start() {
        let mut state = local_state();
        let (a, complete_a) = Writer::controlled();
        let (b, complete_b) = Writer::controlled();
        activate(&mut state, a, "a.tracking");
        state.stop_active();
        activate(&mut state, b, "b.tracking");
        state.stop_active();
        assert_eq!(state.finishing.len(), MAX_WRITERS);
        assert!(matches!(state.can_start(), Err(StartError::WritersBusy)));
        *complete_b.lock() = Some(writer::Completion {
            bytes: 22,
            error: Some("tracking writer: sync: fixture".into()),
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
        let (raw, actions) =
            matching_snapshots(Some(frame.clone()), Some(semantic.clone()), &input);
        assert!(raw.is_none());
        assert!(actions.is_none());
        let current = controller_frame::ControllerFrame {
            sequence: 12,
            ..frame
        };
        assert_eq!(current.display_time_ns, 90);
        let (raw, actions) =
            matching_snapshots(Some(current.clone()), Some(semantic.clone()), &input);
        assert_eq!(raw.unwrap().sequence, 12);
        assert!(actions.is_some());
        let stale = semantic_frame::SemanticFrame {
            published_at: now - Duration::from_nanos(1),
            ..semantic
        };
        let (raw, actions) = matching_snapshots(Some(current), Some(stale), &input);
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
        let captured = capture_publications(publications, started);
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
        let batch = encode_capture(&frame, &captured).unwrap();
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
        assert_eq!(decoded.version, 2);
        assert_eq!(decoded.input, captured);
        assert_eq!(decoded.frames, vec![frame]);
    }
}
