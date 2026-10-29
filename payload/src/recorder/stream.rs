//! The recorder's writer thread: the TCP stream to the host encoder and the per-frame sidecar.
//!
//! The stream is `yuv4mpegpipe`: a one-line text header carrying the size, rate, and chroma
//! layout, then `FRAME\n` followed by the planar bytes for each frame. It is self-describing, so
//! the host side needs no dimensions up front, and every frame is the same size, so a dropped or
//! duplicated frame never desynchronizes the parser. The connection is made from this thread with
//! retries, so the recording can be started before the host listener is up; frames that arrive
//! before the connection exists are dropped and counted.
//!
//! The sidecar lives in the recording's session video folder (see [`crate::session`]): `stream.json` with the
//! stream parameters, and `frames.jsonl` with one line per frame written to the stream, carrying
//! the stream frame index (what the video's frame number is), the capture index from the render
//! thread, the wall clock, the engine's game-run time, whether the frame is a duplicate, and the
//! located HMD views for the frame.
//!
//! Take markers made while the recording runs go into `frames.jsonl` too, as lines with a
//! `marker` object and the `stream_frame` of the first fresh frame captured after the marker. They
//! arrive on their own unbounded channel, so a full frame queue never loses one, and each carries
//! the capture ordinal the render thread was about to assign, which is what places it among the
//! frames regardless of how far the writer lags behind.

use std::{
    io::Write as _,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use parking_lot::Mutex;
use serde::Serialize;

use crate::{
    director::markers::Marker, recorder::RecorderConfig, session::CaptureFolder, vr::CaptureViews,
};

/// What the render thread knows about a frame at capture time.
#[derive(Clone, Copy, Debug)]
pub struct FrameMetadata {
    /// The render thread's capture ordinal, from zero at the start of the recording.
    pub frame: u32,
    pub unix_ms: i64,
    pub game_run_time_us: Option<u64>,
    pub eye: usize,
    /// The stream's luma plane size.
    pub width: u32,
    pub height: u32,
    pub views: Option<CaptureViews>,
}

/// One unit of work for the writer.
pub enum Frame {
    /// Fresh planar bytes for the frame.
    Fresh(Vec<u8>, FrameMetadata),
    /// The readback was not ready: repeat the previous bytes and source metadata.
    Duplicate,
}

/// A take marker on its way to the sidecar.
pub struct PendingMarker {
    /// The capture ordinal of the next frame the render thread captures after the marker: the
    /// marker is written before the first fresh frame whose ordinal is at least this.
    pub capture_frame: u32,
    pub marker: Marker,
}

/// The result of queueing a frame.
pub enum SendOutcome {
    Queued,
    /// The writer's queue is full; the frame is dropped.
    Full,
    /// The writer has failed with this message; the recording should stop.
    Failed(String),
}

/// The writer thread's handle, owned by the recorder state.
pub struct Writer {
    sender: Option<SyncSender<Frame>>,
    markers: Option<Sender<PendingMarker>>,
    pool: Receiver<Vec<u8>>,
    handle: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
}

impl Writer {
    /// Create the sidecar folder and spawn the writer thread, which connects to the host in the
    /// background.
    pub fn spawn(cfg: &RecorderConfig, stamp: &str) -> anyhow::Result<Self> {
        let folder = crate::session::capture_folder(CaptureFolder::Video, stamp)?;
        let sidecar = std::fs::File::create(folder.join("frames.jsonl"))
            .with_context(|| format!("could not create the sidecar in {}", folder.display()))?;
        let target = format!("{}:{}", cfg.host(), cfg.port);
        let frame_rate = cfg.frame_rate.max(1);
        Self::spawn_thread(move |receiver, markers, pool, shared| {
            let mut session = WriterThread {
                target,
                frame_rate,
                folder,
                sidecar: std::io::BufWriter::new(sidecar),
                stream: None,
                next_address: 0,
                header_written: false,
                last_fresh: None,
                stream_frame: 0,
                dropped_before_connect: 0,
                markers,
                pending_markers: Vec::new(),
                pool,
                shared,
            };
            session.run(receiver);
        })
    }

    fn spawn_thread(
        run: impl FnOnce(Receiver<Frame>, Receiver<PendingMarker>, SyncSender<Vec<u8>>, Arc<Shared>)
        + Send
        + 'static,
    ) -> anyhow::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_DEPTH);
        let (marker_sender, marker_receiver) = mpsc::channel();
        let (pool_sender, pool) = mpsc::sync_channel(QUEUE_DEPTH + 2);
        let shared = Arc::new(Shared {
            connected: AtomicBool::new(false),
            error: Mutex::new(None),
            finish_deadline: Mutex::new(None),
        });
        let thread_shared = Arc::clone(&shared);
        let handle = std::thread::Builder::new()
            .name("jc3vr-recorder".to_owned())
            .spawn(move || run(receiver, marker_receiver, pool_sender, thread_shared))
            .context("could not spawn the recorder writer thread")?;
        Ok(Self {
            sender: Some(sender),
            markers: Some(marker_sender),
            pool,
            handle: Some(handle),
            shared,
        })
    }

    /// A buffer for the converter to fill: one the writer has finished with, or a fresh one.
    pub fn take_buffer(&self, len: usize) -> Vec<u8> {
        let mut buffer = self.pool.try_recv().unwrap_or_default();
        buffer.resize(len, 0);
        buffer
    }

    /// Queue a frame without blocking the render thread.
    pub fn send(&self, frame: Frame) -> SendOutcome {
        if let Some(error) = self.shared.error.lock().clone() {
            return SendOutcome::Failed(error);
        }
        let Some(sender) = self.sender.as_ref() else {
            return SendOutcome::Failed("the writer is finishing".to_owned());
        };
        match sender.try_send(frame) {
            Ok(()) => SendOutcome::Queued,
            Err(TrySendError::Full(_)) => SendOutcome::Full,
            Err(TrySendError::Disconnected(_)) => {
                SendOutcome::Failed("the writer thread has exited".to_owned())
            }
        }
    }

    /// Queue a take marker for the sidecar. Never blocks and never drops for want of room; hands
    /// the marker back when the writer is finishing or has failed, so the caller can keep it
    /// elsewhere.
    pub fn send_marker(&self, marker: PendingMarker) -> Result<(), PendingMarker> {
        if self.shared.error.lock().is_some() {
            return Err(marker);
        }
        let Some(sender) = self.markers.as_ref() else {
            return Err(marker);
        };
        sender
            .send(marker)
            .map_err(|mpsc::SendError(marker)| marker)
    }

    /// Whether the thread holds a live connection to the host.
    pub fn connected(&self) -> bool {
        self.shared.connected.load(Ordering::Relaxed)
    }

    /// Close the queue and allow its accepted frames to drain within the network grace period.
    pub fn finish(&mut self) {
        self.markers = None;
        if self.sender.take().is_some() {
            *self.shared.finish_deadline.lock() = Some(Instant::now() + FINISH_GRACE);
        }
    }

    pub(super) fn is_finished(&self) -> bool {
        self.handle.as_ref().is_some_and(JoinHandle::is_finished)
    }

    /// A pending result keeps the join handle owned. I/O failure after exit is safe to reap.
    pub(super) fn poll_completion(&mut self) -> Option<Completion> {
        if !self.is_finished() {
            return None;
        }
        let result = self
            .handle
            .take()
            .expect("recorder: finished handle")
            .join();
        let error = if result.is_err() {
            Some("recorder: the video writer thread panicked".to_owned())
        } else {
            self.shared.error.lock().clone()
        };
        Some(Completion { error })
    }

    #[cfg(test)]
    pub(super) fn spawn_controlled(
        run: impl FnOnce(Receiver<Frame>) -> Result<(), String> + Send + 'static,
    ) -> Self {
        Self::spawn_thread(move |receiver, _markers, _pool, shared| {
            if let Err(error) = run(receiver) {
                *shared.error.lock() = Some(error);
            }
        })
        .expect("recorder: controlled writer spawn")
    }
}

pub(super) struct Completion {
    pub(super) error: Option<String>,
}

/// How many frames may wait for the writer before the render thread starts dropping. At full eye
/// resolution each is around ten megabytes, so this is a bound on memory as much as latency.
const QUEUE_DEPTH: usize = 6;

/// How often the thread retries the host while waiting for the listener.
const CONNECT_RETRY: Duration = Duration::from_millis(500);

const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const WRITE_TIMEOUT: Duration = Duration::from_millis(250);
const FRAME_WRITE_BUDGET: Duration = Duration::from_secs(5);
const FINISH_GRACE: Duration = Duration::from_secs(5);

struct Shared {
    connected: AtomicBool,
    error: Mutex<Option<String>>,
    finish_deadline: Mutex<Option<Instant>>,
}

impl Shared {
    fn network_expired(&self) -> bool {
        self.finish_deadline
            .lock()
            .is_some_and(|deadline| Instant::now() >= deadline)
    }
}

struct WriterThread {
    target: String,
    frame_rate: u32,
    folder: PathBuf,
    sidecar: std::io::BufWriter<std::fs::File>,
    stream: Option<TcpStream>,
    next_address: usize,
    header_written: bool,
    /// The bytes of the last fresh frame, re-sent for duplicates.
    last_fresh: Option<(Vec<u8>, FrameMetadata)>,
    /// The next stream frame index: the frame number the video will have for the next write.
    stream_frame: u32,
    dropped_before_connect: u32,
    markers: Receiver<PendingMarker>,
    /// Markers received but not yet due: their frame has not reached the writer.
    pending_markers: Vec<PendingMarker>,
    pool: SyncSender<Vec<u8>>,
    shared: Arc<Shared>,
}

/// One sidecar line.
#[derive(Serialize)]
struct SidecarLine<'a> {
    stream_frame: u32,
    capture_frame: u32,
    duplicate: bool,
    unix_ms: i64,
    game_run_time_us: Option<u64>,
    eye: usize,
    predicted_display_time_ns: Option<i64>,
    views: Option<[SidecarView<'a>; 2]>,
}

/// One sidecar line for a take marker.
#[derive(Serialize)]
struct SidecarMarkerLine<'a> {
    /// The stream frame the marker falls at: the next frame written after it.
    stream_frame: u32,
    marker: &'a Marker,
}

#[derive(Serialize)]
struct SidecarView<'a> {
    position: &'a [f32; 3],
    orientation: &'a [f32; 4],
    fov: &'a [f32; 4],
}

/// The stream parameters, written once the first frame fixes the size.
#[derive(Serialize)]
struct StreamInfo<'a> {
    width: u32,
    height: u32,
    frame_rate: u32,
    eye: usize,
    pixel_format: &'a str,
    color_space: &'a str,
    color_range: &'a str,
    target: &'a str,
    started_unix_ms: i64,
}

impl WriterThread {
    fn run(&mut self, receiver: Receiver<Frame>) {
        let mut next_attempt = Instant::now();
        for frame in receiver {
            if let Frame::Fresh(_, metadata) = &frame {
                self.write_due_markers(Some(metadata.frame));
            }
            if self.stream.is_none() {
                if Instant::now() >= next_attempt {
                    self.try_connect();
                    next_attempt = Instant::now() + CONNECT_RETRY;
                }
                if self.stream.is_none() {
                    self.dropped_before_connect += 1;
                    if let Frame::Fresh(buffer, _) = frame {
                        let _ = self.pool.try_send(buffer);
                    }
                    continue;
                }
            }
            if let Err(e) = self.write_frame(frame) {
                let message = format!("{e:#}");
                tracing::error!("recorder: writer: {message}");
                *self.shared.error.lock() = Some(message);
                self.shared.connected.store(false, Ordering::Relaxed);
                self.stream = None;
                // Drain the rest so the render thread's sends fail fast rather than block on a
                // full queue, then leave.
                break;
            }
        }
        // Markers made after the last frame the writer saw fall at the end of the stream.
        self.write_due_markers(None);
        if let Some(mut stream) = self.stream.take() {
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        }
        let _ = self.sidecar.flush();
        self.shared.connected.store(false, Ordering::Relaxed);
        if self.dropped_before_connect > 0 {
            tracing::warn!(
                "recorder: {} frames arrived before the host connected and were dropped",
                self.dropped_before_connect
            );
        }
        tracing::info!(
            "recorder: writer finished, {} frames streamed to {}",
            self.stream_frame,
            self.target
        );
    }

    /// Write the markers due before the fresh frame with capture ordinal `capture_frame`, or every
    /// received marker when `None`, at the current stream frame.
    fn write_due_markers(&mut self, capture_frame: Option<u32>) {
        self.pending_markers.extend(self.markers.try_iter());
        let due =
            |pending: &PendingMarker| capture_frame.is_none_or(|c| pending.capture_frame <= c);
        if !self.pending_markers.iter().any(due) {
            return;
        }
        let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending_markers)
            .into_iter()
            .partition(due);
        self.pending_markers = waiting;
        for pending in due {
            let line = SidecarMarkerLine {
                stream_frame: self.stream_frame,
                marker: &pending.marker,
            };
            self.append_sidecar(&line);
        }
    }

    fn try_connect(&mut self) {
        // Hostname resolution can still block in the OS. The retained owner covers that case.
        let result = (|| -> std::io::Result<Option<TcpStream>> {
            let addresses: Vec<_> = self.target.to_socket_addrs()?.collect();
            let deadline = Instant::now() + CONNECT_TIMEOUT;
            let stream = connect_resolved(
                &addresses,
                &mut self.next_address,
                || {
                    if self.shared.network_expired() {
                        Duration::ZERO
                    } else {
                        deadline.saturating_duration_since(Instant::now())
                    }
                },
                |address, remaining| match TcpStream::connect_timeout(address, remaining) {
                    Ok(stream) => Some(stream),
                    Err(error) => {
                        tracing::debug!("recorder: waiting for the host at {address}: {error}");
                        None
                    }
                },
            );
            if let Some(stream) = stream {
                stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
                let _ = stream.set_nodelay(true);
                Ok(Some(stream))
            } else {
                Ok(None)
            }
        })();
        match result {
            Ok(Some(stream)) => {
                tracing::info!("recorder: connected to {}", self.target);
                self.stream = Some(stream);
                self.shared.connected.store(true, Ordering::Relaxed);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::debug!("recorder: waiting for the host at {}: {error}", self.target);
            }
        }
    }

    fn write_frame(&mut self, frame: Frame) -> anyhow::Result<()> {
        // The bytes to send are held outside `self` for the duration so the header and sidecar
        // writes below can borrow `self` freely; they go back into `last_fresh` at the end.
        let (held, metadata, duplicate) = match frame {
            Frame::Fresh(buffer, metadata) => {
                if let Some((previous, _)) = self.last_fresh.take() {
                    let _ = self.pool.try_send(previous);
                }
                (buffer, metadata, false)
            }
            Frame::Duplicate => match self.last_fresh.take() {
                Some((bytes, source)) => (bytes, source, true),
                // Nothing to repeat yet (the ring is still priming): the stream simply starts a
                // frame later.
                None => return Ok(()),
            },
        };
        let result = self.write_bytes(&held, &metadata, duplicate);
        self.last_fresh = Some((held, metadata));
        result
    }

    fn write_bytes(
        &mut self,
        bytes: &[u8],
        metadata: &FrameMetadata,
        duplicate: bool,
    ) -> anyhow::Result<()> {
        let deadline = Instant::now() + FRAME_WRITE_BUDGET;
        if !self.header_written {
            let header = format!(
                "YUV4MPEG2 W{} H{} F{}:1 Ip A1:1 C420jpeg XYSCSS=420JPEG XCOLORRANGE=LIMITED\n",
                metadata.width, metadata.height, self.frame_rate
            );
            write_network(
                self.stream.as_mut().context("no stream")?,
                header.as_bytes(),
                &self.shared,
                deadline,
            )
            .context("writing the stream header")?;
            self.header_written = true;
            self.write_stream_info(metadata);
        }
        let stream_frame = self.stream_frame;
        let stream = self.stream.as_mut().context("no stream")?;
        write_network(stream, b"FRAME\n", &self.shared, deadline)
            .and_then(|()| write_network(stream, bytes, &self.shared, deadline))
            .with_context(|| format!("writing frame {stream_frame} to the host"))?;
        self.write_sidecar_line(metadata, duplicate);
        self.stream_frame += 1;
        Ok(())
    }

    fn write_stream_info(&self, metadata: &FrameMetadata) {
        let info = StreamInfo {
            width: metadata.width,
            height: metadata.height,
            frame_rate: self.frame_rate,
            eye: metadata.eye,
            pixel_format: "yuv420p",
            color_space: "bt709",
            color_range: "limited",
            target: &self.target,
            started_unix_ms: metadata.unix_ms,
        };
        let path = self.folder.join("stream.json");
        let result = serde_json::to_string_pretty(&info)
            .map_err(std::io::Error::other)
            .and_then(|json| std::fs::write(&path, json));
        if let Err(e) = result {
            tracing::warn!("recorder: could not write {}: {e}", path.display());
        }
    }

    fn write_sidecar_line(&mut self, metadata: &FrameMetadata, duplicate: bool) {
        let views = metadata.views.as_ref().map(|v| {
            [0, 1].map(|eye| SidecarView {
                position: &v.eyes[eye].position,
                orientation: &v.eyes[eye].orientation,
                fov: &v.eyes[eye].fov,
            })
        });
        let line = SidecarLine {
            stream_frame: self.stream_frame,
            capture_frame: metadata.frame,
            duplicate,
            unix_ms: metadata.unix_ms,
            game_run_time_us: metadata.game_run_time_us,
            eye: metadata.eye,
            predicted_display_time_ns: metadata.views.map(|v| v.predicted_display_time_ns),
            views,
        };
        self.append_sidecar(&line);
    }

    fn append_sidecar(&mut self, line: &impl Serialize) {
        let result = serde_json::to_writer(&mut self.sidecar, line)
            .map_err(std::io::Error::other)
            .and_then(|()| self.sidecar.write_all(b"\n"));
        if let Err(e) = result {
            tracing::warn!("recorder: could not append to the sidecar: {e}");
        }
    }
}

/// Advance before each attempt so an address that uses the whole budget cannot starve later ones.
fn connect_resolved<T>(
    addresses: &[SocketAddr],
    next_address: &mut usize,
    mut remaining: impl FnMut() -> Duration,
    mut connect: impl FnMut(&SocketAddr, Duration) -> Option<T>,
) -> Option<T> {
    for _ in 0..addresses.len() {
        let budget = remaining();
        if budget.is_zero() {
            break;
        }
        let index = *next_address % addresses.len();
        *next_address = (index + 1) % addresses.len();
        if let Some(stream) = connect(&addresses[index], budget) {
            return Some(stream);
        }
    }
    None
}

/// The socket has a per-call timeout; the deadline also bounds a peer making slow progress.
fn write_network(
    stream: &mut impl std::io::Write,
    mut bytes: &[u8],
    shared: &Shared,
    deadline: Instant,
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= deadline || shared.network_expired() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "recorder: the network drain deadline expired",
            ));
        }
        match stream.write(bytes) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
