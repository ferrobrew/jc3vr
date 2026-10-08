//! The in-process recorder: streams one eye's rendered frames, at the render resolution and the
//! headset's own rate, to a host-side encoder, with a per-frame pose sidecar.
//!
//! ## Pipeline
//!
//! On the render thread, at the point where the per-eye capture textures hold the finished frame
//! (`render_engine_post_draw`, under the engine's context critical section), [`capture_frame`]
//! runs a pixel-shader pass that converts the chosen eye into planar 4:2:0 YUV packed into one
//! `R8` texture ([`convert`]), copies that into a ring of staging textures, and maps the staging
//! texture written two frames earlier with a no-wait map, so the GPU is never stalled for a
//! readback. The mapped bytes are memcpy'd into a buffer and handed to the writer thread.
//!
//! The writer thread ([`stream`]) owns a TCP connection to a host-side listener (an `ffmpeg`
//! reading `yuv4mpegpipe`, see `scripts/capture_host.sh`), writes the y4m stream header once and a
//! frame per buffer, and appends one JSON line per frame to a sidecar in the session folder with
//! the wall clock, the engine's game time, and the located HMD views for that frame.
//!
//! ## Frame accounting
//!
//! Every captured display frame produces exactly one stream frame, so the stream stays constant
//! rate at the headset's refresh and lines up with the sidecar by index. A readback that is not
//! ready (the no-wait map refusing) re-sends the previous frame's bytes and is counted as a
//! duplicate; a writer that cannot keep up drops the frame entirely and is counted as a drop, and
//! the sidecar records both so post can see where the video's timing is not real.
//!
//! ## Threads and shutdown
//!
//! The writer is a joinable thread; [`shutdown`] stops it and waits, on the same argument as the
//! screenshot writer: a thread still running when the DLL unmaps parks the process.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D};

use crate::stereo::{far_phase, single_pass::collapse_active};

pub mod convert;
pub mod stream;
pub(crate) mod tracking;

/// The recorder's settings. Read at the start of a recording; changes apply to the next one.
#[derive(Clone, Serialize, Deserialize)]
pub struct RecorderConfig {
    /// Which eye to record: `0` (left) or `1` (right).
    pub eye: usize,
    /// The output size as a fraction of the eye render size, `0.25..=1.0`. Rounded to even
    /// dimensions.
    pub scale: f32,
    /// The frame rate written into the stream header, which is what the encoder times frames by.
    /// Set to the headset's refresh rate.
    pub frame_rate: u32,
    /// The host listener the frames are streamed to.
    pub host: String,
    pub port: u16,
}
impl RecorderConfig {
    pub const fn new() -> Self {
        Self {
            eye: 0,
            scale: 1.0,
            frame_rate: 72,
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
    /// The stream size once the first frame has been captured.
    pub size: Option<(u32, u32)>,
    pub frames_sent: u32,
    pub frames_duplicated: u32,
    pub frames_dropped: u32,
    /// Whether the writer has a live connection to the host.
    pub connected: bool,
    /// The last writer error, if the recording stopped on one.
    pub error: Option<String>,
    /// The session-relative sidecar folder of the current or last recording.
    pub folder: Option<String>,
}

/// Start a recording with the current config. Any thread. A no-op while one runs.
pub fn start() {
    let cfg = crate::config::Config::lock_query(|c| c.recorder.clone());
    let mut state = STATE.lock();
    if state.recording {
        return;
    }
    if crate::is_shutting_down() {
        return;
    }
    let stamp = crate::session::stamp();
    match stream::Writer::spawn(&cfg, &stamp) {
        Ok(writer) => {
            state.writer = Some(writer);
            state.recording = true;
            state.folder = Some(format!("capture/{stamp}"));
            state.error = None;
            state.size = None;
            state.frame_index = 0;
            state.eye = cfg.eye.min(1);
            state.scale = cfg.scale.clamp(MIN_SCALE, 1.0);
            FRAMES_SENT.store(0, Ordering::Relaxed);
            FRAMES_DUPLICATED.store(0, Ordering::Relaxed);
            FRAMES_DROPPED.store(0, Ordering::Relaxed);
            RECORDING.store(true, Ordering::Release);
            tracing::info!(
                "recorder: recording eye {} at scale {} to {}:{} (sidecar capture/{stamp})",
                state.eye,
                state.scale,
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

/// Stop the recording, if any. Any thread. The writer finishes its queue and closes the stream;
/// the converter is released on the render thread's next pass.
pub fn stop() {
    let mut state = STATE.lock();
    if !state.recording {
        return;
    }
    RECORDING.store(false, Ordering::Release);
    state.recording = false;
    if let Some(mut writer) = state.writer.take() {
        writer.finish();
        // Keep the handle so eject can wait for the drain; a stale one from an earlier recording
        // has long exited and is simply replaced.
        state.finishing = Some(writer);
    }
    tracing::info!(
        "recorder: stopped after {} frames ({} duplicated, {} dropped)",
        FRAMES_SENT.load(Ordering::Relaxed),
        FRAMES_DUPLICATED.load(Ordering::Relaxed),
        FRAMES_DROPPED.load(Ordering::Relaxed)
    );
}

/// Toggle between recording and stopped.
pub fn toggle() {
    if RECORDING.load(Ordering::Acquire) {
        stop();
    } else {
        start();
    }
}

/// Whether a recording is running. Cheap; read from the render thread each frame.
pub fn is_recording() -> bool {
    RECORDING.load(Ordering::Acquire)
}

/// The live status for the debug tab.
pub fn status() -> Status {
    let state = STATE.lock();
    Status {
        recording: state.recording,
        size: state.size,
        frames_sent: FRAMES_SENT.load(Ordering::Relaxed),
        frames_duplicated: FRAMES_DUPLICATED.load(Ordering::Relaxed),
        frames_dropped: FRAMES_DROPPED.load(Ordering::Relaxed),
        connected: state.writer.as_ref().is_some_and(stream::Writer::connected),
        error: state.error.clone(),
        folder: state.folder.clone(),
    }
}

/// The render-thread half, called from `render_engine_post_draw` after the per-eye captures are
/// copied, under the engine's context critical section. `eye_textures` are the per-eye captures
/// and `index` the dispatch that just finished; the chosen eye is captured on the dispatch where
/// its texture holds the new frame. A no-op unless recording.
///
/// # Safety
/// `device` and `context` must be the live engine device and immediate context, and the eye
/// textures live textures on that device whose GPU writes for this frame are complete.
pub unsafe fn capture_frame(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    eye_textures: &[Option<ID3D11Texture2D>; 2],
    index: usize,
) {
    if !is_recording() {
        // Release the GPU resources once the recording ends, on the thread that made them.
        CONVERTER.lock().take();
        return;
    }
    let (eye, scale) = {
        let state = STATE.lock();
        (state.eye, state.scale)
    };
    if far_phase() || !(collapse_active() || index == eye) {
        return;
    }
    let Some(source) = eye_textures[eye].as_ref() else {
        return;
    };
    let mut converter = CONVERTER.lock();
    let converter = match converter.as_mut() {
        Some(c) => c,
        None => match convert::Converter::new(device, source, scale) {
            Ok(c) => {
                let size = c.size();
                STATE.lock().size = Some(size);
                tracing::info!("recorder: stream size {}x{}", size.0, size.1);
                converter.insert(c)
            }
            Err(e) => {
                fail(format!("could not build the converter: {e:#}"));
                return;
            }
        },
    };
    let size = converter.size();
    let mut state = STATE.lock();
    let Some(writer) = state.writer.as_ref() else {
        return;
    };
    let metadata = stream::FrameMetadata {
        frame: state.frame_index,
        unix_ms: jiff::Timestamp::now().as_millisecond(),
        // SAFETY: a read of the clock singleton's counters, which the render thread only reads.
        game_run_time_us: unsafe { jc3gi::clock::Clock::get().map(|c| c.m_GameRunTimeMicro) },
        eye,
        width: size.0,
        height: size.1,
        views: crate::vr::capture_views(),
    };
    let mut buffer = writer.take_buffer(converter.frame_len());
    // SAFETY: forwarded from the caller's contract.
    let frame = match unsafe { converter.convert(device, context, source, &mut buffer) } {
        Ok(true) => stream::Frame::Fresh(buffer, metadata),
        Ok(false) => {
            // Not ready (the ring is still priming, or the map refused): repeat the previous
            // frame so the stream keeps its cadence.
            FRAMES_DUPLICATED.fetch_add(1, Ordering::Relaxed);
            stream::Frame::Duplicate(metadata)
        }
        Err(e) => {
            drop(state);
            fail(format!("readback failed: {e:#}"));
            return;
        }
    };
    match writer.send(frame) {
        stream::SendOutcome::Queued => {
            FRAMES_SENT.fetch_add(1, Ordering::Relaxed);
            state.frame_index += 1;
        }
        stream::SendOutcome::Full => {
            FRAMES_DROPPED.fetch_add(1, Ordering::Relaxed);
        }
        stream::SendOutcome::Failed(e) => {
            drop(state);
            fail(e);
        }
    }
}

/// Stop the recording and wait for the writer to exit, for eject. Returns whether the writer is
/// confirmed stopped; `false` means the payload must stay mapped (see `screenshot::shutdown`).
#[must_use = "a writer that did not finish means the payload must stay mapped"]
pub fn shutdown() -> bool {
    stop();
    let writer = STATE.lock().finishing.take();
    match writer {
        Some(writer) => writer.join(SHUTDOWN_BUDGET),
        None => true,
    }
}

/// The smallest output scale the config accepts.
pub const MIN_SCALE: f32 = 0.25;

/// How long eject waits for the writer to drain and exit.
const SHUTDOWN_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

static RECORDING: AtomicBool = AtomicBool::new(false);
static FRAMES_SENT: AtomicU32 = AtomicU32::new(0);
static FRAMES_DUPLICATED: AtomicU32 = AtomicU32::new(0);
static FRAMES_DROPPED: AtomicU32 = AtomicU32::new(0);

struct State {
    recording: bool,
    writer: Option<stream::Writer>,
    /// The writer of the last recording, kept so eject can join it after `stop` handed it off.
    finishing: Option<stream::Writer>,
    eye: usize,
    scale: f32,
    size: Option<(u32, u32)>,
    frame_index: u32,
    error: Option<String>,
    folder: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State {
    recording: false,
    writer: None,
    finishing: None,
    eye: 0,
    scale: 1.0,
    size: None,
    frame_index: 0,
    error: None,
    folder: None,
});

/// The GPU-side converter, created on the render thread on the first recorded frame and released
/// there once the recording stops.
static CONVERTER: Mutex<Option<convert::Converter>> = Mutex::new(None);

/// Stop the recording on an error, recording it for the debug tab.
fn fail(message: String) {
    tracing::error!("recorder: {message}; stopping");
    STATE.lock().error = Some(message);
    stop();
}
