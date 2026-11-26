//! A streamed take's writer thread: the eye-view frames on their way to the capture host.
//!
//! The render thread converts and reads back each due frame ([`crate::recorder::convert`]) and
//! queues it here as a [`VideoFrame`] without blocking; a full queue refuses the frame, and the
//! render thread owes its stream frames to the next one ([`crate::recorder::pacing`]). The writer
//! receives the connection from the take's thread once the host has confirmed the take
//! ([`crate::recorder::protocol`], version 2), writes the `yuv4mpegpipe` header, and then each frame:
//! the previous image again for each repeat the message carries, then its fresh image. The stream
//! is self-describing and every frame is the same size, so a repeated frame cannot desynchronize
//! the host's parser.
//!
//! Each stream frame gets a line in `frame_map.jsonl` beside the take's sidecar: `stream_frame`,
//! the frame's number in the video, `frame`, the take frame ordinal of the image it shows (the
//! sidecar's `frame`), and `repeat`, whether it repeats the previous stream frame's image.
//!
//! Frames that arrive before the host has confirmed, or after the stream failed, are dropped and
//! counted. When the take ends the render thread's sender is closed; the writer writes what is
//! queued within [`FINISH_GRACE`], closes its half of the connection, which is the stop, and exits.
//! The take's thread then waits for the host to finalize the file.

use std::{
    fs::File,
    io::{BufWriter, Write},
    net::{Shutdown, TcpStream},
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use jc3vr_core::session::RecordingFile;
use parking_lot::Mutex;
use serde::Serialize;

use crate::recorder::protocol::{FRAME_TAG, VideoFormat, stream_header};

/// One message to the writer: `repeats` stream frames of the previous image, then `image`, the
/// planar bytes of a fresh frame with the take frame ordinal it was converted from.
pub(crate) struct VideoFrame {
    pub(crate) repeats: u32,
    pub(crate) image: Option<(Vec<u8>, u32)>,
}

/// Why a frame was not queued.
pub(crate) enum QueueError {
    /// The writer's queue is full; the frame comes back so its buffer can be reused.
    Full(VideoFrame),
    /// The take is finishing or the writer has exited.
    Closed,
}

/// The counters the debug UI reads.
#[derive(Debug, Default)]
pub(crate) struct VideoStats {
    /// Fresh images the render thread queued.
    pub(crate) captured: AtomicU32,
    /// Stream frames written to the host, fresh and repeated.
    pub(crate) written: AtomicU32,
    /// Stream frames that repeat the previous image.
    pub(crate) repeated: AtomicU32,
    /// Frames the queue could not take, or the writer could not send.
    pub(crate) dropped: AtomicU32,
    /// Stream frames given up because the host fell more than a second behind.
    pub(crate) given_up: AtomicU32,
    /// Messages waiting in the queue.
    pub(crate) queued: AtomicU32,
}

/// The render thread's end of a streamed take: the format it publishes, the queue, and the buffer
/// pool. Shared, so the render thread can use it without holding the recorder's state lock.
pub(crate) struct VideoChannel {
    sender: Mutex<Option<SyncSender<VideoFrame>>>,
    pool: Mutex<Receiver<Vec<u8>>>,
    /// Buffers the render thread filled but did not send, kept for its next frame.
    spare: Mutex<Vec<Vec<u8>>>,
    format: OnceLock<VideoFormat>,
    streaming: AtomicBool,
    stats: VideoStats,
}

impl VideoChannel {
    /// Publish the stream's format, which the take's thread waits for before it connects. The
    /// first format published stands.
    pub(crate) fn publish(&self, format: VideoFormat) {
        let _ = self.format.set(format);
    }

    pub(crate) fn format(&self) -> Option<&VideoFormat> {
        self.format.get()
    }

    /// Whether the writer holds a confirmed connection, so frames sent now reach the host.
    pub(crate) fn streaming(&self) -> bool {
        self.streaming.load(Ordering::Acquire)
    }

    /// A buffer of `len` bytes for the converter to fill: one the writer has finished with, or a
    /// fresh one.
    pub(crate) fn take_buffer(&self, len: usize) -> Vec<u8> {
        let spare = self.spare.lock().pop();
        let mut buffer = spare
            .or_else(|| self.pool.lock().try_recv().ok())
            .unwrap_or_default();
        buffer.resize(len, 0);
        buffer
    }

    /// Keep a buffer that was not sent for the next frame.
    pub(crate) fn recycle(&self, buffer: Vec<u8>) {
        let mut spare = self.spare.lock();
        if spare.len() < MAX_SPARE {
            spare.push(buffer);
        }
    }

    /// Queue a frame without blocking.
    pub(crate) fn send(&self, frame: VideoFrame) -> Result<(), QueueError> {
        let sender = self.sender.lock();
        let Some(sender) = sender.as_ref() else {
            return Err(QueueError::Closed);
        };
        match sender.try_send(frame) {
            Ok(()) => {
                self.stats.queued.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(TrySendError::Full(frame)) => Err(QueueError::Full(frame)),
            Err(TrySendError::Disconnected(_)) => Err(QueueError::Closed),
        }
    }

    pub(crate) fn stats(&self) -> &VideoStats {
        &self.stats
    }

    /// Close the queue: the writer writes what is queued and ends the stream.
    pub(crate) fn close(&self) {
        self.sender.lock().take();
    }
}

/// The writer thread, owned by the take's thread.
pub(crate) struct Writer {
    channel: Arc<VideoChannel>,
    connection: Sender<TcpStream>,
    handle: Option<JoinHandle<()>>,
    shared: Arc<WriterShared>,
}

impl Writer {
    /// Create `frame_map.jsonl` in `folder` and spawn the writer, which waits for the format and
    /// the connection.
    pub(crate) fn spawn(folder: &Path) -> anyhow::Result<Self> {
        let path = folder.join(RecordingFile::FrameMap.name());
        let sidecar = File::create(&path).with_context(|| {
            format!(
                "recorder: could not create the video sidecar at {}",
                path.display()
            )
        })?;
        Self::spawn_with(BufWriter::new(sidecar))
    }

    fn spawn_with(sidecar: impl Write + Send + 'static) -> anyhow::Result<Self> {
        let (sender, frames) = mpsc::sync_channel(QUEUE_DEPTH);
        let (pool_sender, pool) = mpsc::sync_channel(QUEUE_DEPTH + 2);
        let (connection, connections) = mpsc::channel();
        let channel = Arc::new(VideoChannel {
            sender: Mutex::new(Some(sender)),
            pool: Mutex::new(pool),
            spare: Mutex::new(Vec::new()),
            format: OnceLock::new(),
            streaming: AtomicBool::new(false),
            stats: VideoStats::default(),
        });
        let shared = Arc::new(WriterShared::default());
        let thread = WriterThread {
            channel: Arc::clone(&channel),
            shared: Arc::clone(&shared),
            connections,
            pool: pool_sender,
            sidecar,
            sidecar_failed: false,
            stream: None,
            header_written: false,
            last: None,
            stream_frame: 0,
            finish_deadline: None,
        };
        let handle = std::thread::Builder::new()
            .name("jc3vr-recorder-video".to_owned())
            .spawn(move || thread.run(frames))
            .context("recorder: could not spawn the video writer thread")?;
        Ok(Self {
            channel,
            connection,
            handle: Some(handle),
            shared,
        })
    }

    pub(crate) fn channel(&self) -> &Arc<VideoChannel> {
        &self.channel
    }

    /// Hand the writer the host's confirmed connection.
    pub(crate) fn connect(&self, stream: TcpStream) {
        // A writer that has exited has nothing left to send; the take's thread closes the stream.
        let _ = self.connection.send(stream);
    }

    /// Stop sending: the host refused or dropped the take.
    pub(crate) fn host_closed(&self) {
        self.shared.host_closed.store(true, Ordering::Release);
    }

    /// Wait up to `budget` for the writer to finish the stream and exit. Returns whether it did;
    /// the stream's failure, if any, is in [`Self::error`].
    pub(crate) fn finish(&mut self, budget: Duration) -> bool {
        self.channel.close();
        let deadline = Instant::now() + budget;
        while self
            .handle
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
        {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(FINISH_POLL);
        }
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            self.shared
                .error
                .lock()
                .get_or_insert_with(|| "the video writer thread panicked".to_owned());
        }
        true
    }

    /// Whether any frame reached the host.
    pub(crate) fn wrote_frames(&self) -> bool {
        self.channel.stats.written.load(Ordering::Relaxed) > 0
    }

    /// The stream's failure, if it failed.
    pub(crate) fn error(&self) -> Option<String> {
        self.shared.error.lock().clone()
    }
}

/// How many frames may wait for the writer before the render thread starts dropping. A side-by-side
/// frame is about twelve megabytes, so this bounds memory as much as latency.
const QUEUE_DEPTH: usize = 4;
/// How many unsent buffers the render thread keeps.
const MAX_SPARE: usize = 2;
/// How long the writer keeps writing queued frames after the take ends.
const FINISH_GRACE: Duration = Duration::from_secs(5);
/// How long one frame's write may take before the host counts as stalled.
const FRAME_WRITE_BUDGET: Duration = Duration::from_secs(5);
/// How long a write the socket's buffer could not take waits before retrying. The socket is
/// non-blocking, since Winsock leaves a timed-out blocking send in an indeterminate state.
const WOULD_BLOCK_RETRY: Duration = Duration::from_millis(1);
/// How often the writer checks for the connection while no frame arrives.
const IDLE_POLL: Duration = Duration::from_millis(50);
const FINISH_POLL: Duration = Duration::from_millis(10);

#[derive(Default)]
struct WriterShared {
    host_closed: AtomicBool,
    error: Mutex<Option<String>>,
}

struct WriterThread<W> {
    channel: Arc<VideoChannel>,
    shared: Arc<WriterShared>,
    connections: Receiver<TcpStream>,
    pool: SyncSender<Vec<u8>>,
    sidecar: W,
    sidecar_failed: bool,
    stream: Option<TcpStream>,
    header_written: bool,
    /// The last fresh image and its take frame ordinal, re-sent for repeats.
    last: Option<(Vec<u8>, u32)>,
    /// The next stream frame's number in the video.
    stream_frame: u32,
    /// Set once the queue closes: writes stop at this time.
    finish_deadline: Option<Instant>,
}

#[derive(Serialize)]
struct FrameLine {
    stream_frame: u32,
    frame: u32,
    repeat: bool,
}

impl<W: Write> WriterThread<W> {
    fn run(mut self, frames: Receiver<VideoFrame>) {
        loop {
            match frames.recv_timeout(IDLE_POLL) {
                Ok(frame) => {
                    self.channel.stats.queued.fetch_sub(1, Ordering::Relaxed);
                    self.accept_connection();
                    self.write(frame);
                }
                Err(RecvTimeoutError::Timeout) => self.accept_connection(),
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if self.finish_deadline.is_none() && self.channel.sender.lock().is_none() {
                self.finish_deadline = Some(Instant::now() + FINISH_GRACE);
            }
        }
        self.channel.streaming.store(false, Ordering::Release);
        if let Some(stream) = self.stream.take() {
            // The end of the stream is the stop: the host finalizes the file on it.
            let _ = stream.shutdown(Shutdown::Write);
        }
        if !self.sidecar_failed
            && let Err(error) = self.sidecar.flush()
        {
            self.fail(format!("could not write frame_map.jsonl: {error}"));
        }
        tracing::info!(
            "recorder: the video writer finished after {} stream frames",
            self.stream_frame
        );
    }

    fn accept_connection(&mut self) {
        if self.stream.is_some() || self.channel.format.get().is_none() {
            return;
        }
        if let Ok(stream) = self.connections.try_recv() {
            self.stream = Some(stream);
            self.channel.streaming.store(true, Ordering::Release);
        }
    }

    fn write(&mut self, frame: VideoFrame) {
        let usable = self.stream.is_some()
            && !self.shared.host_closed.load(Ordering::Acquire)
            && self.shared.error.lock().is_none();
        if !usable {
            self.channel.stats.dropped.fetch_add(1, Ordering::Relaxed);
            if let Some((buffer, _)) = frame.image {
                let _ = self.pool.try_send(buffer);
            }
            return;
        }
        if let Err(error) = self.write_frame(frame) {
            self.channel.streaming.store(false, Ordering::Release);
            self.fail(format!("capture host: {error:#}"));
        }
    }

    fn write_frame(&mut self, frame: VideoFrame) -> anyhow::Result<()> {
        let deadline = self.deadline();
        if !self.header_written {
            let format = *self
                .channel
                .format
                .get()
                .context("the stream has no format")?;
            let stream = self.stream.as_mut().context("no stream")?;
            write_nonblocking(stream, stream_header(&format).as_bytes(), deadline)
                .context("could not send the stream header")?;
            self.header_written = true;
        }
        for _ in 0..frame.repeats {
            match self.last.take() {
                Some(last) => {
                    let result = self.send(&last.0, last.1, true, deadline);
                    self.last = Some(last);
                    result?;
                    self.channel.stats.repeated.fetch_add(1, Ordering::Relaxed);
                }
                // Nothing to repeat yet: the stream starts with the first fresh image.
                None => break,
            }
        }
        if let Some((buffer, source)) = frame.image {
            let result = self.send(&buffer, source, false, deadline);
            if let Some((previous, _)) = self.last.replace((buffer, source)) {
                let _ = self.pool.try_send(previous);
            }
            result?;
        }
        Ok(())
    }

    fn send(
        &mut self,
        bytes: &[u8],
        source: u32,
        repeat: bool,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        let stream_frame = self.stream_frame;
        let stream = self.stream.as_mut().context("no stream")?;
        write_nonblocking(stream, FRAME_TAG, deadline)
            .and_then(|()| write_nonblocking(stream, bytes, deadline))
            .with_context(|| format!("could not send stream frame {stream_frame}"))?;
        self.stream_frame += 1;
        self.channel.stats.written.fetch_add(1, Ordering::Relaxed);
        self.append(&FrameLine {
            stream_frame,
            frame: source,
            repeat,
        });
        Ok(())
    }

    fn deadline(&self) -> Instant {
        let budget = Instant::now() + FRAME_WRITE_BUDGET;
        self.finish_deadline
            .map_or(budget, |finish| finish.min(budget))
    }

    fn append(&mut self, line: &FrameLine) {
        if self.sidecar_failed {
            return;
        }
        let result = serde_json::to_writer(&mut self.sidecar, line)
            .map_err(std::io::Error::other)
            .and_then(|()| self.sidecar.write_all(b"\n"));
        if let Err(error) = result {
            self.sidecar_failed = true;
            tracing::error!("recorder: could not write frame_map.jsonl: {error}");
        }
    }

    fn fail(&self, message: String) {
        tracing::error!("recorder: video: {message}");
        self.shared.error.lock().get_or_insert(message);
    }
}

/// Write all of `bytes` to the non-blocking `stream` by `deadline`.
fn write_nonblocking(
    stream: &mut impl Write,
    mut bytes: &[u8],
    deadline: Instant,
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the host stopped reading the stream",
            ));
        }
        match stream.write(bytes) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(WOULD_BLOCK_RETRY);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
