//! The session recorder's video half: a take's per-frame sidecar, the eye views streamed out of the
//! game, and the control connection to the capture host that encodes them.
//!
//! ## What records what
//!
//! The video is encoded outside the game process, by a capture host listening on a TCP port: the
//! injector, which stays running after it injects and drives an ffmpeg it carries, or on Linux the
//! relay (`scripts/capture_host.sh`, which runs `tools/capture/relay.py`). For each take the payload
//! connects and names the take's folder ([`protocol`]).
//!
//! With the in-process capture on (`RecorderConfig::in_process`, the default) the payload streams
//! the eye views itself (protocol version 2). On the render thread, once both eyes of a frame are
//! final, [`capture_frame`] draws them side by side into one planar-YUV frame on the GPU, each eye
//! scaled to the configured width with its aspect kept ([`layout`], [`convert`]), and reads the
//! frame back through a staging ring without stalling. The take's video writer ([`stream`]) sends
//! the frames to the host as `yuv4mpegpipe`, paced to a constant rate by the wall clock
//! ([`pacing`]), and the host encodes them with its audio into the take's folder.
//!
//! With it off, the host records the game's mirror window on screen with the desktop's audio
//! (protocol version 1, which only the relay speaks). What the video shows is then what the desktop
//! mirror shows (`jc3vr_view::vr::mirror`), at the window's size, so the mirror view decides what is
//! recorded and the window must be on screen.
//!
//! The payload writes the take's sidecar, `frames.jsonl`: one line per rendered frame with the wall
//! clock, the engine's game time, and the located HMD views, plus lines for take markers
//! ([`mark`]) and for the mirror's layout whenever it changes ([`note_mirror`]). The host writes
//! `stream.json` beside it with the wall-clock time of the video's first frame, which is what lines
//! the two up.
//!
//! ## Threads and shutdown
//!
//! Each take has one thread ([`take`]) that writes the sidecar and services the connection, and a
//! streamed take a video writer thread beside it, which the take's thread joins. Active
//! and finishing takes share a bounded owner; completed threads are joined before their slots are
//! reused. [`shutdown`] waits for every retained thread and keeps the DLL mapped if any has not
//! exited.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use jc3vr_core::{config::ConfigSlot, session::CaptureFolder};
use jc3vr_view::vr::MirrorFrame;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};

use crate::{
    director::markers::Marker,
    recorder::{
        convert::{Converter, Readback},
        layout::{DEFAULT_EYE_WIDTH, StreamLayout},
        owners::{Completion, Takes},
        pacing::{Backlog, Pacer},
        protocol::{VideoFormat, WindowHint},
        stream::{QueueError, VideoChannel, VideoFrame},
        take::{FrameRecord, Record, Take, TakeRequest},
    },
};

mod convert;
pub(crate) mod layout;
mod owners;
mod pacing;
pub(crate) mod protocol;
pub mod session_recording;
mod stream;
mod take;

pub use crate::recorder::{
    layout::{Arrangement, MAX_EYE_WIDTH, MIN_EYE_WIDTH},
    protocol::Region,
};

/// The video recorder's settings, at `recorder` in the settings file; the payload binds the slot at
/// startup.
pub static RECORDER_CONFIG: ConfigSlot<RecorderConfig> = ConfigSlot::new("recorder");

/// The recorder's settings. Read at the start of a recording; changes apply to the next one.
#[derive(Clone, Serialize, Deserialize)]
pub struct RecorderConfig {
    /// The capture host to connect to.
    pub host: String,
    pub port: u16,
    /// Stream the eye views from inside the game (protocol version 2). Off, the capture host
    /// records the desktop mirror window instead, which only the relay does.
    pub in_process: bool,
    /// The width each eye is scaled to in the streamed frame, keeping its aspect.
    pub eye_width: u32,
    /// How the two eyes share the streamed frame.
    pub arrangement: Arrangement,
    /// The streamed frame rate, held by the wall clock whatever the game renders at.
    pub frame_rate: u32,
}
impl RecorderConfig {
    pub const fn new() -> Self {
        Self {
            host: String::new(),
            port: 47172,
            in_process: true,
            eye_width: DEFAULT_EYE_WIDTH,
            arrangement: Arrangement::SideBySide,
            frame_rate: DEFAULT_FRAME_RATE,
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

impl Default for RecorderConfig {
    fn default() -> Self {
        Self::new()
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
    /// The running take's eye-view stream, when it streams them.
    pub video: Option<VideoStatus>,
}

/// A streamed take's state for the debug UI.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoStatus {
    /// The frame size, once the first frame has fixed it.
    pub size: Option<(u32, u32)>,
    pub frame_rate: u32,
    pub arrangement: Arrangement,
    /// Fresh images queued to the writer.
    pub captured: u32,
    /// Stream frames sent to the host, fresh and repeated.
    pub written: u32,
    /// Stream frames that repeat the previous image.
    pub repeated: u32,
    /// Frames the writer's queue could not take, or the writer could not send.
    pub dropped: u32,
    /// Stream frames given up because the host fell more than a second behind.
    pub given_up: u32,
    /// Frames waiting in the writer's queue.
    pub queued: u32,
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
    /// The screen region recorded, for a mirror take.
    pub region: Option<Region>,
    pub output: Option<String>,
    /// The wall-clock time of the video's first frame, when the host knew it at the start.
    pub video_start_unix_ms: Option<f64>,
    /// The video encoder, for a streamed take.
    pub encoder: Option<String>,
    /// What the host records the audio from, or why it records none.
    pub audio: Option<String>,
}

/// Start a recording whose folder is stamped `stamp`, so a session recording's tracking half lands
/// in the same folder. Any thread. A no-op while one runs.
pub(crate) fn start_stamped(stamp: &str) {
    if is_recording() || jc3vr_core::is_shutting_down() {
        return;
    }
    let cfg = RECORDER_CONFIG.lock().clone();
    // Read before the state lock: neither call takes it, and the title read sends no message to the
    // window's thread.
    let window = WindowHint {
        title: jc3vr_view::vr::window::title(),
        client_size: jc3vr_view::vr::window::client_size().map(|(w, h)| [w, h]),
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
            folder: jc3vr_core::session::capture_folder(CaptureFolder::Recording, stamp)?,
            host_folder: jc3vr_core::session::host_capture_folder(CaptureFolder::Recording, stamp),
            target: format!("{}:{}", cfg.host(), cfg.port),
            window,
            stream: cfg.in_process,
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
            state.stream = cfg.in_process.then(|| StreamSettings {
                eye_width: cfg.eye_width,
                arrangement: cfg.arrangement,
                frame_rate: cfg.frame_rate.clamp(MIN_FRAME_RATE, MAX_FRAME_RATE),
            });
            STREAMING.store(cfg.in_process, Ordering::Release);
            RECORDING.store(true, Ordering::Release);
            if cfg.in_process {
                tracing::info!(
                    "recorder: recording the eye views to {folder}, {} wide each, {:?}, at {} fps, \
                     capture host {}:{}",
                    cfg.eye_width,
                    cfg.arrangement,
                    cfg.frame_rate,
                    cfg.host(),
                    cfg.port
                );
            } else {
                tracing::info!(
                    "recorder: recording the mirror window to {folder}, capture host {}:{}",
                    cfg.host(),
                    cfg.port
                );
            }
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
    STREAMING.store(false, Ordering::Release);
    state.recording = false;
    if let Some(video) = state.takes.active().and_then(Take::video) {
        let stats = video.stats();
        tracing::info!(
            "recorder: the stream sent {} frames ({} repeated) from {} captured; {} dropped, {} \
             given up",
            stats.written.load(Ordering::Relaxed),
            stats.repeated.load(Ordering::Relaxed),
            stats.captured.load(Ordering::Relaxed),
            stats.dropped.load(Ordering::Relaxed),
            stats.given_up.load(Ordering::Relaxed),
        );
    }
    state.takes.stop();
    tracing::info!("recorder: stopped after {} frames", state.frame_index);
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

/// Whether a running take streams the eye views, so both eyes' captures must be copied every frame
/// even where the eyes go straight to the headset. Cheap; read from the render thread each frame.
pub fn wants_eye_captures() -> bool {
    STREAMING.load(Ordering::Acquire)
}

/// Whether a running take has the capture host record the mirror window, so the mirror's frame
/// stamp and layout records mean something.
pub fn records_mirror() -> bool {
    is_recording() && !wants_eye_captures()
}

/// The live status for the debug tab.
pub fn status() -> Status {
    let mut state = STATE.lock();
    state.reap();
    let active = state.takes.active().filter(|_| state.recording);
    let video = active
        .and_then(Take::video)
        .zip(state.stream)
        .map(|(channel, settings)| video_status(channel, settings));
    Status {
        recording: state.recording,
        host: active.map(Take::host),
        lines: active.map_or(0, Take::lines),
        error: active.and_then(Take::error).or_else(|| state.error.clone()),
        folder: state.folder.clone(),
        video,
    }
}

/// Note a rendered frame, from `render_engine_post_draw` on the frame's last near dispatch, when
/// every eye image of the frame is final. Returns the frame's take ordinal, which a streamed take's
/// [`capture_frame`] tags its image with. A no-op unless recording.
pub fn frame_rendered() -> Option<u32> {
    crate::recorder::session_recording::poll();
    if !is_recording() {
        return None;
    }
    let unix_ms = jiff::Timestamp::now().as_millisecond();
    // SAFETY: a read of the clock singleton's counters, which the render thread only reads.
    let game_run_time_us = unsafe { jc3gi::clock::Clock::get().map(|c| c.m_GameRunTimeMicro) };
    let views = jc3vr_view::vr::capture_views();
    let mut state = STATE.lock();
    let frame = state.frame_index;
    let take = state.takes.active().filter(|_| state.recording)?;
    let sent = take.send(Record::Frame(Box::new(FrameRecord {
        frame,
        unix_ms,
        game_run_time_us,
        views,
    })));
    if sent.is_ok() {
        state.frame_index = frame.saturating_add(1);
        Some(frame)
    } else {
        drop(state);
        fail("the take thread has exited".to_owned());
        None
    }
}

/// The render thread's half of a streamed take, called from `render_engine_post_draw` right after
/// [`frame_rendered`] on the frame's last near dispatch, when both eyes' captures hold the frame,
/// under the engine's context critical section. `frame` is the ordinal [`frame_rendered`] returned,
/// or `None` outside a take. Draws both eyes into the stream's frame when the pacing says one is
/// due, and queues the frame read back from an earlier capture. A no-op unless a streamed take
/// runs; releases the GPU resources once none does.
///
/// # Safety
/// `device` and `context` must be the live engine device and immediate context, and `eyes` live
/// textures on that device whose GPU writes for this frame are submitted.
pub unsafe fn capture_frame(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    eyes: [&ID3D11Texture2D; 2],
    frame: Option<u32>,
) {
    let active = frame.and_then(|frame| {
        let state = STATE.lock();
        let channel = state
            .takes
            .active()
            .filter(|_| state.recording)
            .and_then(Take::video)
            .cloned()?;
        Some((channel, state.stream?, frame))
    });
    // `fail` takes only the state lock, so holding this one across it cannot deadlock.
    let mut guard = CAPTURE.lock();
    let Some((channel, settings, frame)) = active else {
        // Release the GPU resources on the thread that made them.
        guard.take();
        return;
    };
    let capture = match guard.take() {
        Some(capture) if Arc::ptr_eq(&capture.channel, &channel) => capture,
        _ => match StreamCapture::new(device, eyes, channel, settings) {
            Ok(built) => built,
            Err(error) => {
                fail(format!("could not set up the eye-view capture: {error:#}"));
                return;
            }
        },
    };
    let capture = guard.insert(capture);
    // SAFETY: forwarded from the caller's contract.
    if let Err(error) = unsafe { capture.capture(device, context, eyes, frame) } {
        fail(format!("the eye-view capture failed: {error:#}"));
    }
}

/// Release the eye-view capture's GPU resources once no streamed take runs. Render thread, from
/// the post-draw on frames that do not capture.
pub fn release_capture() {
    if !wants_eye_captures() {
        CAPTURE.lock().take();
    }
}

/// Note the desktop mirror's layout after a present, so the sidecar records it whenever it changes.
/// A no-op unless a take records the mirror window.
pub fn note_mirror(mirror: MirrorFrame) {
    if !records_mirror() {
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
/// into the window (`stamped`). A no-op unless a take records the mirror window.
pub fn note_present(present: u32, stamped: bool) {
    if !records_mirror() {
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

/// The default streamed frame rate. The game renders at about 40 fps in the headset, so a fixed 30
/// keeps the stream at a rate the game can meet.
pub(crate) const DEFAULT_FRAME_RATE: u32 = 30;
/// The slowest streamed frame rate the config accepts.
pub const MIN_FRAME_RATE: u32 = 10;
/// The fastest streamed frame rate the config accepts.
pub const MAX_FRAME_RATE: u32 = 90;

/// How long eject waits for the take's thread to finish with the host and exit.
const SHUTDOWN_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

static RECORDING: AtomicBool = AtomicBool::new(false);
static STREAMING: AtomicBool = AtomicBool::new(false);

/// A streamed take's settings, fixed at its start.
#[derive(Clone, Copy, Debug)]
struct StreamSettings {
    eye_width: u32,
    arrangement: Arrangement,
    frame_rate: u32,
}

fn video_status(channel: &VideoChannel, settings: StreamSettings) -> VideoStatus {
    let stats = channel.stats();
    let format = channel.format();
    VideoStatus {
        size: format.map(|format| (format.layout.width, format.layout.height)),
        frame_rate: format.map_or(settings.frame_rate, |format| format.frame_rate),
        arrangement: settings.arrangement,
        captured: stats.captured.load(Ordering::Relaxed),
        written: stats.written.load(Ordering::Relaxed),
        repeated: stats.repeated.load(Ordering::Relaxed),
        dropped: stats.dropped.load(Ordering::Relaxed),
        given_up: stats.given_up.load(Ordering::Relaxed),
        queued: stats.queued.load(Ordering::Relaxed),
    }
}

/// A streamed take's render-thread state: the converter built for its layout, and its pacing.
struct StreamCapture {
    channel: Arc<VideoChannel>,
    converter: Converter,
    frame_rate: u32,
    pacer: Pacer,
    backlog: Backlog,
    /// When the stream's first frame was due, once the host is ready for frames.
    origin: Option<Instant>,
}

impl StreamCapture {
    /// Build the converter for the eyes' current size and publish the stream's format, which the
    /// take's thread waits for before it connects to the host.
    fn new(
        device: &ID3D11Device,
        eyes: [&ID3D11Texture2D; 2],
        channel: Arc<VideoChannel>,
        settings: StreamSettings,
    ) -> anyhow::Result<Self> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: a live capture texture.
        unsafe { eyes[0].GetDesc(&mut desc) };
        let layout = StreamLayout::new(
            desc.Width,
            desc.Height,
            settings.eye_width,
            settings.arrangement,
        )?;
        let converter = Converter::new(device, eyes, layout)?;
        tracing::info!(
            "recorder: streaming {}x{} at {} fps ({:?}) from {}x{} eyes",
            layout.width,
            layout.height,
            settings.frame_rate,
            layout.arrangement,
            desc.Width,
            desc.Height
        );
        channel.publish(VideoFormat {
            layout,
            frame_rate: settings.frame_rate,
        });
        Ok(Self {
            channel,
            converter,
            frame_rate: settings.frame_rate,
            pacer: Pacer::new(settings.frame_rate),
            backlog: Backlog::new(settings.frame_rate),
            origin: None,
        })
    }

    /// # Safety
    /// As [`capture_frame`].
    unsafe fn capture(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        eyes: [&ID3D11Texture2D; 2],
        frame: u32,
    ) -> anyhow::Result<()> {
        // Frames converted before the host is ready would only be dropped.
        if !self.channel.streaming() {
            return Ok(());
        }
        let now = Instant::now();
        let origin = *self.origin.get_or_insert(now);
        let due = self.pacer.due(now - origin);
        if due == 0 {
            return Ok(());
        }
        let stats = self.channel.stats();
        let mut buffer = self
            .channel
            .take_buffer(self.converter.layout().frame_len());
        // SAFETY: forwarded from the caller's contract.
        let readback = unsafe {
            self.converter
                .convert(device, context, eyes, &mut buffer, frame)?
        };
        let image = match readback {
            Readback::Fresh(source) => Some((buffer, source)),
            Readback::Priming | Readback::Pending => {
                self.channel.recycle(buffer);
                None
            }
        };
        let fresh = u32::from(image.is_some());
        let message = VideoFrame {
            repeats: due - fresh + self.backlog.take(),
            image,
        };
        let frames = message.repeats + fresh;
        match self.channel.send(message) {
            Ok(()) => {
                stats.captured.fetch_add(fresh, Ordering::Relaxed);
            }
            Err(QueueError::Full(refused)) => {
                if let Some((buffer, _)) = refused.image {
                    self.channel.recycle(buffer);
                }
                stats.dropped.fetch_add(1, Ordering::Relaxed);
                let given_up = self.backlog.owe(frames);
                if given_up > 0 {
                    let total = stats.given_up.fetch_add(given_up, Ordering::Relaxed) + given_up;
                    // Once a second of video at most: the backlog stays at its limit until the
                    // host catches up.
                    if total / self.frame_rate != (total - given_up) / self.frame_rate {
                        tracing::warn!(
                            "recorder: the capture host is more than a second behind; {total} \
                             stream frames given up so far, so the video runs short of the take"
                        );
                    }
                }
            }
            Err(QueueError::Closed) => anyhow::bail!("the video writer has exited"),
        }
        Ok(())
    }
}

/// The streamed take's render-thread state, created on the first captured frame and released on
/// the render thread once no streamed take runs.
static CAPTURE: Mutex<Option<StreamCapture>> = Mutex::new(None);

struct State {
    recording: bool,
    takes: Takes,
    frame_index: u32,
    /// The mirror layout the running take's sidecar last recorded.
    last_mirror: Option<MirrorFrame>,
    /// The running take's stream settings, when it streams the eye views.
    stream: Option<StreamSettings>,
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
    stream: None,
    error: None,
    folder: None,
});

/// Stop the recording on an error, recording it for the debug tab.
fn fail(message: String) {
    tracing::error!("recorder: {message}; stopping");
    STATE.lock().error = Some(format!("recorder: {message}"));
    stop();
}
