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
//!
//! Beside each frame the recorder writes the mod-side [`events`] noted since the previous one. The
//! [`lookback`] keeps the last few seconds of the same capture batches while motion controllers are
//! enabled, whether or not a recording runs, and saves them as a recording on request.

pub(crate) mod events;
pub(crate) mod format;
pub(crate) mod lookback;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod tests;
mod writer;

use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use tracking_format::{
    event::{Event, Label, Settings},
    input::{CapturedInput, DiscontinuityReason, InputRecord},
};

use crate::{
    config::Config,
    controllers::frame as controller_frame,
    recorder::tracking::{
        events::{EventKind, Noted},
        format::{
            ControllerSample, EyeSample, Frame, HeadSample, Header, RecenterEvent, SemanticSample,
            SessionSample, StartMarker,
        },
        lookback::{Entry, Lookback, LookbackStatus},
        writer::{CaptureBatch, SendOutcome, Writer},
    },
    session::{RecordingFile, SessionError},
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
    /// prevent durability. The chunked writer stores chunks; the raw writer stores original records.
    pub(crate) bytes: u64,
    /// Preamble and original record bytes for complete accepted batches, before compression.
    pub(crate) logical_bytes: u64,
    /// How long the current or last recording ran.
    pub(crate) duration: Duration,
    /// Why the last recording did not start or stopped early.
    pub(crate) error: Option<String>,
}

/// Why a recording, or a save of the lookback, did not start.
#[derive(Debug)]
pub(crate) enum StartError {
    /// The lookback holds no frames to save.
    LookbackEmpty,
    /// The test tools are switched off.
    Disabled,
    /// A replay is driving the game, so there is nothing live to record.
    Replaying,
    /// There is no player character to mark the start at.
    NoPlayer,
    /// The recording's session path could not be produced.
    Session(SessionError),
    Writer(writer::WriterError),
    /// The bounded owner set is full of writers that have not exited.
    WritersBusy,
    ShuttingDown,
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "recorder: tracking: ")?;
        match self {
            StartError::LookbackEmpty => write!(f, "lookback: the window holds no frames"),
            StartError::Disabled => write!(f, "the test tools are switched off (testing.enabled)"),
            StartError::Replaying => write!(f, "a replay is running"),
            StartError::NoPlayer => write!(f, "there is no player character to mark the start at"),
            StartError::Session(e) => write!(f, "{e}"),
            StartError::Writer(e) => write!(f, "{e}"),
            StartError::WritersBusy => write!(f, "previous recording writers are still finishing"),
            StartError::ShuttingDown => write!(f, "the recorder is shutting down"),
        }
    }
}

impl std::error::Error for StartError {}

/// Ask for a recording to start at the top of the next frame. Any thread.
pub(crate) fn request_start() {
    *REQUESTED_STAMP.lock() = None;
    REQUEST.store(REQUEST_START, Ordering::Relaxed);
}

/// Ask for a recording stamped `stamp` to start at the top of the next frame, so it pairs with a
/// video recording of the same stamp. Any thread.
pub(crate) fn request_start_stamped(stamp: String) {
    *REQUESTED_STAMP.lock() = Some(stamp);
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

/// The recorder's state after collecting completed writers.
pub(crate) fn status() -> TrackingStatus {
    let mut state = STATE.lock();
    state.reap();
    state.snapshot()
}

/// The recorder's state without polling, joining, or collecting completed writers.
pub(crate) fn peek_status() -> TrackingStatus {
    STATE.lock().snapshot()
}

/// The lookback's state after collecting completed writers.
pub(crate) fn lookback_status() -> LookbackStatus {
    let mut state = STATE.lock();
    state.reap();
    let mut status = state.lookback_save.clone();
    if let Some(lookback) = &state.lookback {
        status.running = true;
        status.frames = lookback.ring.len();
        status.seconds = lookback.ring.span().as_secs_f32();
        status.bytes = lookback.ring.bytes();
    }
    status
}

/// Service a start or stop request, and keep the lookback. Game thread, once per frame, outside
/// the VR runtime lock.
pub(crate) fn frame_tick() {
    STATE.lock().reap();
    match REQUEST.swap(REQUEST_NONE, Ordering::Relaxed) {
        REQUEST_START => start(),
        REQUEST_STOP => stop(),
        _ => {}
    }
    lookback_tick();
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

/// Record one frame, into the running recording and the lookback. Called by the frame loop on the
/// game thread under the runtime lock, after the views are located and the controllers are
/// sampled; it takes only leaf locks. A no-op unless either collects.
pub(crate) fn capture(sample: CaptureSample) {
    let publications = history::take_capture();
    let noted = events::take();
    if !is_recording() && !lookback::is_running() {
        return;
    }
    let captured_at = Instant::now();
    let controllers = controller_frame::latest();
    let semantic = semantic_frame::latest();
    let session_state = crate::vr::published_status().session_state;
    let stage = stage::stage();
    let mut guard = STATE.lock();
    let state = &mut *guard;
    let captured = Captured {
        sample,
        captured_at,
        publications: &publications,
        noted: &noted,
        controllers: controllers.as_ref(),
        semantic: semantic.as_ref(),
        session_state,
        stage,
        recenters: std::mem::take(&mut state.recenters),
    };
    if let Some(lookback) = state.lookback.as_mut() {
        match captured.batch(lookback.epoch, lookback.hand_joints) {
            Ok(batch) => lookback.ring.push(Entry {
                time_ns: batch.capture_time_ns,
                bytes: batch.bytes.into(),
                input_records: batch.input_records,
                transform: lookback.transform,
            }),
            Err(error) => tracing::warn!("recorder: tracking: lookback: {error}; frame left out"),
        }
    }
    let Some(active) = state.active.as_mut() else {
        return;
    };
    let batch = match captured.batch(active.started, active.hand_joints) {
        Ok(batch) => batch,
        Err(error) => {
            let error = error.to_string();
            drop(guard);
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
            drop(guard);
            fail(&error);
        }
    }
}

/// What one capture read, for encoding against each collector's time origin.
struct Captured<'a> {
    sample: CaptureSample,
    captured_at: Instant,
    publications: &'a [Publication],
    noted: &'a [Noted],
    controllers: Option<&'a controller_frame::ControllerFrame>,
    semantic: Option<&'a semantic_frame::SemanticFrame>,
    session_state: Option<openxr::SessionState>,
    stage: stage::StageInfo,
    recenters: Vec<RecenterEvent>,
}

impl Captured<'_> {
    /// The frame, its input records, and its events, timed from `origin`.
    fn batch(
        &self,
        origin: Instant,
        hand_joints: bool,
    ) -> Result<CaptureBatch, format::DecodeError> {
        let input = capture_publications(self.publications, origin);
        let (controllers, semantic) = matching_snapshots(self.controllers, self.semantic, &input);
        let frame = Frame {
            time_ns: nanos_since(self.captured_at, origin),
            display_time_ns: self.sample.display_time_ns,
            session: SessionSample {
                state: self.session_state,
                should_render: self.sample.should_render,
                head_tracked: self.sample.head_tracked,
            },
            head: self.sample.eyes.map(|eyes| HeadSample { eyes }),
            stage: self.stage,
            controllers: controllers.map(|controller| {
                let mut hands = controller.hands.clone();
                if !hand_joints {
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
                global: semantic.global.clone(),
                mode: semantic.mode.clone(),
            }),
            recenters: self.recenters.clone(),
        };
        encode_capture(&frame, &input, &events::stamp(self.noted, origin))
    }
}

fn matching_snapshots<'a>(
    controllers: Option<&'a controller_frame::ControllerFrame>,
    semantic: Option<&'a semantic_frame::SemanticFrame>,
    input: &[CapturedInput],
) -> (
    Option<&'a controller_frame::ControllerFrame>,
    Option<&'a semantic_frame::SemanticFrame>,
) {
    let controllers = controllers.filter(|frame| {
        input
            .iter()
            .any(|entry| entry.frame_sequence == Some(frame.sequence))
    });
    let semantic = semantic.filter(|semantic| {
        controllers.is_some_and(|frame| frame.published_at == semantic.published_at)
    });
    (controllers, semantic)
}

fn capture_publications(publications: &[Publication], origin: Instant) -> Vec<CapturedInput> {
    publications
        .iter()
        .filter(|entry| entry.observed_at >= origin)
        .map(|entry| CapturedInput {
            capture_time_ns: nanos_since(entry.observed_at, origin),
            frame_sequence: entry.frame_sequence,
            record: entry.record.clone(),
        })
        .collect()
}

pub(super) fn nanos_since(at: Instant, origin: Instant) -> u64 {
    u64::try_from(at.saturating_duration_since(origin).as_nanos()).unwrap_or(u64::MAX)
}

fn encode_capture(
    frame: &Frame,
    input: &[CapturedInput],
    events: &[Event],
) -> Result<CaptureBatch, format::DecodeError> {
    let mut bytes = Vec::new();
    let mut append = |encoded: Vec<u8>| {
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
        Ok(())
    };
    for event in events {
        append(tracking_format::encode_event(event)?)?;
    }
    let mut input_records = 0u64;
    for entry in input {
        input_records = input_records.saturating_add(match &entry.record {
            InputRecord::Group(_) | InputRecord::Policy(_) => 1,
            InputRecord::Discontinuity { discarded, .. } => *discarded,
        });
        append(tracking_format::encode_input(entry)?)?;
    }
    append(format::encode_frame(frame))?;
    Ok(CaptureBatch {
        bytes,
        capture_time_ns: frame.time_ns,
        input_records,
    })
}

/// Note a recenter of the cockpit baseline, recorded on the next frame. Game thread.
pub(crate) fn note_recenter(event: RecenterEvent) {
    if events::collecting() {
        STATE.lock().recenters.push(event);
    }
}

/// Close every writer without waiting on disk. The eject caller must pin the payload when any
/// retained owner is still running. I/O failure after termination does not prevent unmapping.
#[must_use = "a writer that did not finish means the payload must stay mapped"]
pub(crate) fn shutdown() -> bool {
    let mut state = STATE.lock();
    REQUEST.store(REQUEST_NONE, Ordering::Relaxed);
    RECORDING.store(false, Ordering::Relaxed);
    state.lookback = None;
    lookback::set_running(false);
    state.shutdown()
}

/// Includes the active writer, and leaves room for a lookback save beside it. Saturation refuses a
/// start rather than discarding an owner.
const MAX_WRITERS: usize = 3;

/// How close to a director bookmark the player must stand for the start marker to name it.
const BOOKMARK_RADIUS_M: f32 = 2.0;

const REQUEST_NONE: u8 = 0;
const REQUEST_START: u8 = 1;
const REQUEST_STOP: u8 = 2;

static REQUEST: AtomicU8 = AtomicU8::new(REQUEST_NONE);
/// The stamp a pending start request asks for; a fresh one when `None`.
static REQUESTED_STAMP: Mutex<Option<String>> = Mutex::new(None);
static RECORDING: AtomicBool = AtomicBool::new(false);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static DROPPED: AtomicU32 = AtomicU32::new(0);

struct State {
    active: Option<Active>,
    finishing: Vec<Finishing>,
    shutting_down: bool,
    status: TrackingStatus,
    /// Recenters since the last captured frame.
    recenters: Vec<RecenterEvent>,
    lookback: Option<Lookback>,
    /// The last lookback save's file and error; the window's own fields stay default.
    lookback_save: LookbackStatus,
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
}

impl State {
    const fn idle() -> Self {
        Self {
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
            recenters: Vec::new(),
            lookback: None,
            lookback_save: LookbackStatus {
                running: false,
                frames: 0,
                seconds: 0.0,
                bytes: 0,
                path: None,
                error: None,
            },
        }
    }

    fn snapshot(&self) -> TrackingStatus {
        let mut status = self.status.clone();
        status.recording = is_recording();
        status.frames = FRAMES.load(Ordering::Relaxed);
        status.dropped = DROPPED.load(Ordering::Relaxed);
        if let Some(active) = &self.active {
            status.bytes = active.writer.bytes_written();
            status.logical_bytes = active.writer.logical_bytes_written();
            status.duration = active.started.elapsed();
        }
        status
    }

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
                    if self.lookback_save.path.as_ref() == Some(&writer.path) {
                        self.lookback_save.error = Some(error.clone());
                    }
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

static STATE: Mutex<State> = Mutex::new(State::idle());

fn start() {
    let stamp = REQUESTED_STAMP
        .lock()
        .take()
        .unwrap_or_else(crate::session::stamp);
    if is_recording() {
        return;
    }
    match begin(stamp) {
        Ok(path) => tracing::info!("recorder: tracking: recording to {}", path.display()),
        Err(error) => {
            tracing::warn!("{error}");
            STATE.lock().status.error = Some(error.to_string());
        }
    }
}

fn begin(stamp: String) -> Result<PathBuf, StartError> {
    if !Config::lock_query(|c| c.testing.enabled) {
        return Err(StartError::Disabled);
    }
    if crate::testing::replay::is_active() {
        return Err(StartError::Replaying);
    }
    let transform = crate::director::teleport::player_transform().ok_or(StartError::NoPlayer)?;
    let hand_joints = Config::lock_query(|c| c.testing.record_hand_joints);
    let header = header_at(transform, stamp.clone());
    let path = crate::session::recording_file(RecordingFile::Tracking, &stamp)
        .map_err(StartError::Session)?;
    let mut state = STATE.lock();
    state.can_start()?;
    let writer = Writer::start(&path, &header).map_err(StartError::Writer)?;
    discard_before_start();
    state.active = Some(Active {
        writer,
        path: path.clone(),
        started: Instant::now(),
        hand_joints,
    });
    state.status = TrackingStatus {
        path: Some(path.clone()),
        ..TrackingStatus::default()
    };
    FRAMES.store(0, Ordering::Relaxed);
    DROPPED.store(0, Ordering::Relaxed);
    RECORDING.store(true, Ordering::Relaxed);
    drop(state);
    if let Some(toml) = settings_toml() {
        crate::controllers::diagnostics::note_settings(&toml);
    }
    if let Some(name) = &header.start.bookmark {
        tracing::info!("recorder: tracking: the start marker is at the bookmark {name:?}");
    }
    Ok(path)
}

/// The header of a recording whose start marker is `transform`, stamped `stamp`.
fn header_at(transform: [f32; 16], stamp: String) -> Header {
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
    let (dominant_hand, world_scale, time_scale) = Config::lock_query(|c| {
        (
            c.controllers.dominant_hand,
            c.vr.world_scale,
            c.director.time_scale,
        )
    });
    Header {
        build: crate::BUILD_STAMP.to_owned(),
        recorded_at: stamp,
        action_schema: crate::vr::input::semantic::SCHEMA_VERSION,
        start: StartMarker { bookmark, ..marker },
        dominant_hand,
        world_scale,
        time_scale,
    }
}

fn discard_before_start() {
    history::begin_capture();
}

/// Run the lookback while motion controllers are enabled, the OpenXR runtime is up, and no replay
/// drives the game, and service a save request. Without a runtime no frame is captured, so the
/// player's transform is not read for one.
fn lookback_tick() {
    let (enabled, hand_joints) =
        Config::lock_query(|c| (c.controllers.enabled, c.testing.record_hand_joints));
    let run =
        enabled && crate::vr::published_status().runtime_up && !crate::testing::replay::is_active();
    let transform = run
        .then(crate::director::teleport::player_transform)
        .flatten();
    let save = lookback::take_save_request();
    {
        let mut state = STATE.lock();
        if run && !state.shutting_down {
            let lookback = state
                .lookback
                .get_or_insert_with(|| Lookback::new(hand_joints));
            lookback.hand_joints = hand_joints;
            lookback.transform = transform;
        } else {
            state.lookback = None;
        }
        lookback::set_running(state.lookback.is_some());
    }
    if save {
        match save_lookback() {
            Ok(path) => {
                tracing::info!("recorder: tracking: lookback: saving to {}", path.display());
                STATE.lock().lookback_save = LookbackStatus {
                    path: Some(path),
                    ..LookbackStatus::default()
                };
            }
            Err(error) => {
                tracing::warn!("{error}");
                STATE.lock().lookback_save.error = Some(error.to_string());
            }
        }
    }
}

/// Write the lookback's window to a new recording on a writer thread of its own.
fn save_lookback() -> Result<PathBuf, StartError> {
    let snapshot = STATE
        .lock()
        .lookback
        .as_ref()
        .and_then(|lookback| lookback.ring.snapshot())
        .ok_or(StartError::LookbackEmpty)?;
    let transform = snapshot
        .transform
        .or_else(crate::director::teleport::player_transform)
        .ok_or(StartError::NoPlayer)?;
    let stamp = crate::session::stamp();
    let header = header_at(transform, stamp.clone());
    let path = crate::session::recording_file(RecordingFile::Lookback, &stamp)
        .map_err(StartError::Session)?;
    // The evicted batches took their input history with them: say so before the first record.
    let discontinuity = snapshot
        .evicted_inputs
        .map(|discarded| {
            tracking_format::encode_input(&CapturedInput {
                capture_time_ns: snapshot.first_time_ns,
                frame_sequence: None,
                record: InputRecord::Discontinuity {
                    reason: DiscontinuityReason::Overflow,
                    discarded,
                },
            })
        })
        .transpose()
        .map_err(|error| StartError::Writer(writer::WriterError::Encode(error)))?;
    // The window may have evicted the settings a recording opens with, so the save opens with them.
    let settings = settings_toml()
        .map(|toml| {
            tracking_format::encode_event(&Event {
                time_ns: snapshot.first_time_ns,
                kind: EventKind::Settings(Box::new(Settings {
                    toml: Label::Owned(toml),
                })),
            })
        })
        .transpose()
        .map_err(|error| StartError::Writer(writer::WriterError::Encode(error)))?;
    let lead = match (settings, discontinuity) {
        (None, None) => None,
        (settings, discontinuity) => Some(
            settings
                .into_iter()
                .chain(discontinuity)
                .flatten()
                .collect::<Vec<u8>>(),
        ),
    };
    let mut state = STATE.lock();
    state.can_start()?;
    let writer =
        Writer::dump(&path, &header, lead, snapshot.batches).map_err(StartError::Writer)?;
    state.finishing.push(Finishing {
        writer,
        path: path.clone(),
    });
    Ok(path)
}

/// The settings as their file holds them, for the recording to open with.
fn settings_toml() -> Option<String> {
    match toml::to_string(&crate::config::get()) {
        Ok(toml) => Some(toml),
        Err(error) => {
            tracing::warn!("recorder: tracking: the settings did not serialize: {error}");
            None
        }
    }
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
