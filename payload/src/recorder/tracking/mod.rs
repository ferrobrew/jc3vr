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

use crate::{
    config::Config,
    controllers::frame as controller_frame,
    recorder::tracking::{
        format::{
            ControllerSample, EyeSample, Frame, HeadSample, Header, RecenterEvent, SemanticSample,
            SessionSample, StartMarker,
        },
        writer::{SendOutcome, Writer},
    },
    vr::{input::semantic::frame as semantic_frame, stage},
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
    pub(crate) bytes: u64,
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
    let state = STATE.lock();
    let mut status = state.status.clone();
    status.recording = is_recording();
    status.frames = FRAMES.load(Ordering::Relaxed);
    status.dropped = DROPPED.load(Ordering::Relaxed);
    if let Some(active) = &state.active {
        status.bytes = active.writer.bytes_written();
        status.duration = active.started.elapsed();
    }
    status
}

/// Service a start or stop request. Game thread, once per frame, outside the VR runtime lock.
pub(crate) fn frame_tick() {
    match REQUEST.swap(REQUEST_NONE, Ordering::Relaxed) {
        REQUEST_START => start(),
        REQUEST_STOP => stop(),
        _ => {}
    }
    // A writer that failed on its own is noticed here even on a frame no sample reaches it.
    let failed = STATE
        .lock()
        .active
        .as_ref()
        .and_then(|active| active.writer.error());
    if let Some(error) = failed {
        fail(&error);
    }
}

/// Record one frame. Called by the frame loop on the game thread under the runtime lock, after
/// the views are located and the controllers are sampled; it takes only leaf locks. A no-op
/// unless recording.
pub(crate) fn capture(sample: CaptureSample) {
    if !is_recording() {
        return;
    }
    let controllers = controller_frame::latest().map(|frame| ControllerSample {
        focused: frame.focused,
        action_set: frame.action_set,
        hands: frame.hands,
    });
    let semantic = semantic_frame::latest().map(|frame| SemanticSample {
        action_set: frame.action_set,
        global: frame.global,
        mode: frame.mode,
    });
    let session_state = crate::vr::published_status().session_state;
    let stage = stage::stage();
    let mut state = STATE.lock();
    let Some(active) = state.active.as_mut() else {
        return;
    };
    let frame = Frame {
        time_ns: u64::try_from(active.started.elapsed().as_nanos()).unwrap_or(u64::MAX),
        display_time_ns: sample.display_time_ns,
        session: SessionSample {
            state: session_state,
            should_render: sample.should_render,
            head_tracked: sample.head_tracked,
        },
        head: sample.eyes.map(|eyes| HeadSample { eyes }),
        stage,
        controllers: controllers.map(|mut c| {
            if !active.hand_joints {
                for hand in &mut c.hands {
                    hand.joints = None;
                }
            }
            c
        }),
        semantic,
        recenters: std::mem::take(&mut active.recenters),
    };
    match active.writer.send(format::encode_frame(&frame)) {
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

/// Note a recenter of the cockpit baseline, recorded on the next frame. Game thread.
pub(crate) fn note_recenter(event: RecenterEvent) {
    if !is_recording() {
        return;
    }
    if let Some(active) = STATE.lock().active.as_mut() {
        active.recenters.push(event);
    }
}

/// Stop the recording and wait for the writer to exit, for eject. Returns whether the writer is
/// confirmed stopped; `false` means the payload must stay mapped.
#[must_use = "a writer that did not finish means the payload must stay mapped"]
pub(crate) fn shutdown() -> bool {
    stop();
    let writer = STATE.lock().finishing.take();
    writer.is_none_or(|writer| writer.join(SHUTDOWN_BUDGET))
}

/// How long eject waits for the writer to drain.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(10);

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
    /// The writer of the last recording, kept so eject can wait for its drain.
    finishing: Option<Writer>,
    status: TrackingStatus,
}

struct Active {
    writer: Writer,
    started: Instant,
    hand_joints: bool,
    /// Recenters since the last recorded frame.
    recenters: Vec<RecenterEvent>,
}

static STATE: Mutex<State> = Mutex::new(State {
    active: None,
    finishing: None,
    status: TrackingStatus {
        recording: false,
        path: None,
        frames: 0,
        dropped: 0,
        bytes: 0,
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
    let (dominant_hand, world_scale, time_scale, hand_joints) = Config::lock_query(|c| {
        (
            c.controllers.dominant_hand,
            c.vr.world_scale,
            c.director.time_scale,
            c.testing.record_hand_joints,
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
    let writer =
        Writer::spawn(&path, &format::encode_preamble(&header)).map_err(StartError::Writer)?;
    let mut state = STATE.lock();
    state.active = Some(Active {
        writer,
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

fn stop() {
    let mut state = STATE.lock();
    let Some(mut active) = state.active.take() else {
        return;
    };
    RECORDING.store(false, Ordering::Relaxed);
    state.status.bytes = active.writer.bytes_written();
    state.status.duration = active.started.elapsed();
    active.writer.finish();
    let unfinished = state.finishing.replace(active.writer);
    drop(state);
    // Two recordings stopped in quick succession: wait for the older one here rather than drop a
    // handle to a thread that may still be writing.
    if let Some(writer) = unfinished {
        let _ = writer.join(SHUTDOWN_BUDGET);
    }
    tracing::info!(
        frames = FRAMES.load(Ordering::Relaxed),
        dropped = DROPPED.load(Ordering::Relaxed),
        "recorder: tracking: stopped"
    );
}

fn fail(error: &str) {
    tracing::error!("recorder: tracking: {error}; stopping");
    stop();
    STATE.lock().status.error = Some(error.to_owned());
}
