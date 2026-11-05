//! The session recorder's video half: a take's per-frame sidecar, and the control connection that
//! has the capture host record the game's mirror window.
//!
//! ## What records what
//!
//! The video is recorded outside the game process. The capture host (`scripts/capture_host.sh`,
//! which runs `tools/capture/relay.py`) listens on a TCP port; for each take the payload connects,
//! names the take's folder and the game window ([`protocol`]), and the host finds that window in the
//! compositor and records its region on screen, with the desktop's audio, into the take's folder.
//! What the video shows is therefore what the desktop mirror shows (`crate::vr::mirror`), at the
//! window's size, so the mirror view decides what is recorded and the window must be on screen.
//!
//! The payload writes the take's sidecar, `frames.jsonl`: one line per rendered frame with the wall
//! clock, the engine's game time, and the located HMD views, plus lines for take markers
//! ([`mark`]) and for the mirror's layout whenever it changes ([`note_mirror`]). The host writes
//! `stream.json` beside it with the wall-clock time of the video's first frame, which is what lines
//! the two up.
//!
//! ## Threads and shutdown
//!
//! Each take has one thread ([`take`]) that writes the sidecar and services the connection. Active
//! and finishing takes share a bounded owner; completed threads are joined before their slots are
//! reused. [`shutdown`] waits for every retained thread and keeps the DLL mapped if any has not
//! exited.

use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    director::markers::Marker,
    recorder::{
        owners::{Completion, Takes},
        protocol::WindowHint,
        take::{FrameRecord, Record, Take, TakeRequest},
    },
    session::CaptureFolder,
    vr::MirrorView,
};

mod owners;
pub(crate) mod protocol;
pub(crate) mod session_recording;
mod take;
pub(crate) mod tracking;

pub use crate::recorder::protocol::Region;

/// The recorder's settings. Read at the start of a recording; changes apply to the next one.
#[derive(Clone, Serialize, Deserialize)]
pub struct RecorderConfig {
    /// The capture host to connect to.
    pub host: String,
    pub port: u16,
}
impl RecorderConfig {
    pub const fn new() -> Self {
        Self {
            host: String::new(),
            port: 47172,
        }
    }

    /// The host to connect to, defaulting to loopback when unset.
    pub fn host(&self) -> &str {
        if self.host.trim().is_empty() {
            "127.0.0.1"
        } else {
            self.host.trim()
        }
    }
}

/// A live snapshot of the recorder for the debug tab.
#[derive(Clone, Debug)]
pub struct Status {
    pub recording: bool,
    /// The capture host's state for the running take, or `None` when none runs.
    pub host: Option<HostState>,
    /// The sidecar lines the running take has written.
    pub lines: u32,
    /// The last take's error, if the host refused or dropped it or the sidecar failed.
    pub error: Option<String>,
    /// The session-relative folder of the current or last recording.
    pub folder: Option<String>,
}

/// Where a take's connection to the capture host stands.
#[derive(Clone, Debug, PartialEq)]
pub enum HostState {
    /// Waiting for the host to listen.
    Waiting,
    /// Connected; waiting for the host to confirm it records.
    Starting,
    /// The host records.
    Capturing(Capture),
    /// The take stopped; waiting for the host to finalize the video.
    Finishing,
    /// The connection is over: the take finished, or the host refused or dropped it.
    Closed,
}

/// What the host reported when it started recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    /// The video file, as the host names it.
    pub file: String,
    pub region: Region,
    pub output: Option<String>,
    /// The wall-clock time of the video's first frame, when the host knew it at the start.
    pub video_start_unix_ms: Option<f64>,
}

/// The desktop mirror's layout, as the sidecar records it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MirrorFrame {
    pub view: MirrorView,
    /// The window's client size.
    pub window: (u32, u32),
    /// Where each eye's image lands in the window, in on-screen order.
    pub draws: [Option<MirrorDraw>; 2],
}

/// One eye's image in the mirror window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MirrorDraw {
    pub eye: usize,
    /// The whole eye image's rectangle (`x, y, width, height`) in window pixels. It extends past the
    /// window where the mirror crops the image.
    pub rect: [f32; 4],
}

/// Start a recording with the current config. Any thread. A no-op while one runs.
pub fn start() {
    start_stamped(&crate::session::stamp());
}

/// Start a recording whose folder is stamped `stamp`, so a session recording's tracking half lands
/// in the same folder. Any thread. A no-op while one runs.
pub fn start_stamped(stamp: &str) {
    if is_recording() || crate::is_shutting_down() {
        return;
    }
    let cfg = crate::config::Config::lock_query(|c| c.recorder.clone());
    // Read before the state lock: neither call takes it, and the title read sends no message to the
    // window's thread.
    let window = WindowHint {
        title: crate::vr::window::title(),
        client_size: crate::vr::window::client_size().map(|(w, h)| [w, h]),
    };
    let mut state = STATE.lock();
    state.reap();
    if state.recording {
        return;
    }
    let folder = CaptureFolder::Recording
        .relative(stamp)
        .display()
        .to_string();
    let spawn = || {
        Take::spawn(TakeRequest {
            stamp: stamp.to_owned(),
            folder: crate::session::capture_folder(CaptureFolder::Recording, stamp)?,
            host_folder: crate::session::host_capture_folder(CaptureFolder::Recording, stamp),
            target: format!("{}:{}", cfg.host(), cfg.port),
            window,
        })
    };
    let (completions, result) = state.takes.start(folder.clone(), spawn);
    state.report(completions);
    match result {
        Ok(()) => {
            state.recording = true;
            state.folder = Some(folder.clone());
            state.error = None;
            state.frame_index = 0;
            state.last_mirror = None;
            RECORDING.store(true, Ordering::Release);
            tracing::info!(
                "recorder: recording to {folder}, capture host {}:{}",
                cfg.host(),
                cfg.port
            );
        }
        Err(e) => {
            state.error = Some(format!("{e:#}"));
            tracing::error!("recorder: could not start: {e:#}");
        }
    }
}

/// Stop the recording, if any. Any thread. The take's thread flushes the sidecar and has the host
/// finalize the video.
pub fn stop() {
    let mut state = STATE.lock();
    state.reap();
    if !state.recording {
        return;
    }
    RECORDING.store(false, Ordering::Release);
    state.recording = false;
    state.takes.stop();
    tracing::info!("recorder: stopped after {} frames", state.frame_index);
}

/// Toggle between recording and stopped.
pub fn toggle() {
    if is_recording() {
        stop();
    } else {
        start();
    }
}

/// Put a take marker into the running recording's sidecar, before the next rendered frame. Any
/// thread. Hands the marker back when no recording is running (or its take is finishing), so the
/// caller can record it elsewhere.
pub fn mark(marker: Marker) -> Result<(), Marker> {
    let state = STATE.lock();
    let Some(take) = state.takes.active().filter(|_| state.recording) else {
        return Err(marker);
    };
    take.send(Record::Marker {
        frame: state.frame_index,
        marker,
    })
    .map_err(|record| match record {
        Record::Marker { marker, .. } => marker,
        _ => unreachable!("recorder: a refused marker comes back as a marker"),
    })
}

/// Whether a recording is running. Cheap; read from the render thread each frame.
pub fn is_recording() -> bool {
    RECORDING.load(Ordering::Acquire)
}

/// The live status for the debug tab.
pub fn status() -> Status {
    let mut state = STATE.lock();
    state.reap();
    let active = state.takes.active().filter(|_| state.recording);
    Status {
        recording: state.recording,
        host: active.map(Take::host),
        lines: active.map_or(0, Take::lines),
        error: active.and_then(Take::error).or_else(|| state.error.clone()),
        folder: state.folder.clone(),
    }
}

/// Note a rendered frame, from `render_engine_post_draw` on the frame's last near dispatch, when
/// every eye image of the frame is final. A no-op unless recording.
pub fn frame_rendered() {
    if !is_recording() {
        return;
    }
    let unix_ms = jiff::Timestamp::now().as_millisecond();
    // SAFETY: a read of the clock singleton's counters, which the render thread only reads.
    let game_run_time_us = unsafe { jc3gi::clock::Clock::get().map(|c| c.m_GameRunTimeMicro) };
    let views = crate::vr::capture_views();
    let mut state = STATE.lock();
    let frame = state.frame_index;
    let Some(take) = state.takes.active().filter(|_| state.recording) else {
        return;
    };
    let sent = take.send(Record::Frame(Box::new(FrameRecord {
        frame,
        unix_ms,
        game_run_time_us,
        views,
    })));
    if sent.is_ok() {
        state.frame_index = frame.saturating_add(1);
    } else {
        drop(state);
        fail("the take thread has exited".to_owned());
    }
}

/// Note the desktop mirror's layout after a present, so the sidecar records it whenever it changes.
/// A no-op unless recording.
pub fn note_mirror(mirror: MirrorFrame) {
    if !is_recording() {
        return;
    }
    let mut state = STATE.lock();
    if state.last_mirror == Some(mirror) {
        return;
    }
    let frame = state.frame_index;
    let Some(take) = state.takes.active().filter(|_| state.recording) else {
        return;
    };
    let record = Record::Mirror {
        frame,
        unix_ms: jiff::Timestamp::now().as_millisecond(),
        mirror,
    };
    if take.send(record).is_ok() {
        state.last_mirror = Some(mirror);
    }
}

/// Note a present of the desktop mirror: its counter `present`, and whether the frame stamp drew it
/// into the window (`stamped`). A no-op unless recording.
pub fn note_present(present: u32, stamped: bool) {
    if !is_recording() {
        return;
    }
    let state = STATE.lock();
    let frame = state.frame_index;
    let Some(take) = state.takes.active().filter(|_| state.recording) else {
        return;
    };
    // A present lost to a full or closed channel is only a gap in the counter's log; the take's own
    // failure is reported by the frame records.
    let _ = take.send(Record::Present {
        frame,
        unix_ms: jiff::Timestamp::now().as_millisecond(),
        present,
        stamped,
    });
}

/// Stop the recording and wait for the take's thread to exit, for eject. Returns whether every take
/// thread is confirmed stopped; `false` means the payload must stay mapped (see
/// `image_writer::shutdown`).
#[must_use = "a take thread that did not finish means the payload must stay mapped"]
pub fn shutdown() -> bool {
    stop();
    let mut state = STATE.lock();
    let result = state.takes.shutdown(SHUTDOWN_BUDGET);
    state.report(result.completions);
    if !result.stopped {
        tracing::error!(
            "recorder: take threads have not exited; leaving the payload mapped rather than \
             unmapping under a live thread"
        );
    }
    result.stopped
}

/// How long eject waits for the take's thread to finish with the host and exit.
const SHUTDOWN_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

static RECORDING: AtomicBool = AtomicBool::new(false);

struct State {
    recording: bool,
    takes: Takes,
    frame_index: u32,
    /// The mirror layout the running take's sidecar last recorded.
    last_mirror: Option<MirrorFrame>,
    error: Option<String>,
    folder: Option<String>,
}

impl State {
    fn reap(&mut self) {
        let completions = self.takes.reap();
        self.report(completions);
    }

    fn report(&mut self, completions: Vec<Completion>) {
        for completion in completions {
            if self.folder.as_ref() == Some(&completion.folder) && completion.error.is_some() {
                self.error = completion.error;
            }
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    recording: false,
    takes: Takes::new(),
    frame_index: 0,
    last_mirror: None,
    error: None,
    folder: None,
});

/// Stop the recording on an error, recording it for the debug tab.
fn fail(message: String) {
    tracing::error!("recorder: {message}; stopping");
    STATE.lock().error = Some(format!("recorder: {message}"));
    stop();
}
