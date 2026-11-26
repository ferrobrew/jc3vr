//! One take's thread: the per-frame sidecar and the control connection to the capture host.
//!
//! The thread owns `frames.jsonl` in the take's folder and appends one JSON line per [`Record`] in
//! the order the records were sent, so take markers and mirror layout changes land between the
//! frames they fall between. Between records it services the connection to the capture host
//! ([`crate::recorder::protocol`]): it connects with retries until the host listens, sends the start
//! line, and reads the host's answers without blocking, publishing the host's state for the debug
//! UI. When the recorder drops the record channel the thread flushes the sidecar, sends the stop
//! line, waits a bounded time for the host to confirm the video is finalized, and exits.
//!
//! The thread retries the connection only until the first one succeeds. A host that refuses the take
//! or drops the connection ends the video for this take, and the error says why; the sidecar keeps
//! being written until the take stops.
//!
//! A streamed take (protocol version 2) also owns a video writer ([`crate::recorder::stream`]). The
//! thread connects only once the render thread has published the stream's format, which the start
//! line carries, and hands the writer the connection once the host confirms. At the end it lets the
//! writer finish the stream, whose end is the stop, before it waits for the host to finalize.

use std::{
    fs::File,
    io::{BufWriter, Read as _, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use jc3vr_core::session::RecordingFile;
use jc3vr_view::vr::{CaptureEyeView, CaptureViews, MirrorDraw, MirrorFrame, MirrorView};
use parking_lot::Mutex;
use serde::Serialize;

use crate::{
    director::markers::Marker,
    recorder::{
        Capture, HostState,
        protocol::{
            HostMessage, LineBuffer, WindowHint, parse_host_line, start_line, stop_line,
            stream_start_line,
        },
        stream::{VideoChannel, Writer},
    },
};

/// What a take's thread needs at spawn.
pub(crate) struct TakeRequest {
    /// The take's stamp, which names its folder.
    pub(crate) stamp: String,
    /// The take's folder as the payload opens it.
    pub(crate) folder: PathBuf,
    /// The take's folder as the host names it, when known.
    pub(crate) host_folder: Option<String>,
    /// The capture host's `host:port`.
    pub(crate) target: String,
    pub(crate) window: WindowHint,
    /// Whether the payload streams the eye views itself (protocol version 2) rather than having the
    /// host record the mirror window (version 1).
    pub(crate) stream: bool,
}

/// One sidecar line.
pub(crate) enum Record {
    /// A rendered frame. Boxed, since it carries both eyes' views.
    Frame(Box<FrameRecord>),
    /// A take marker, before frame `frame`.
    Marker { frame: u32, marker: Marker },
    /// The mirror's layout from frame `frame` on.
    Mirror {
        frame: u32,
        unix_ms: i64,
        mirror: MirrorFrame,
    },
    /// A present of the mirror after frame `frame`: its counter, and whether the frame stamp drew
    /// it into the window.
    Present {
        frame: u32,
        unix_ms: i64,
        present: u32,
        stamped: bool,
    },
}

/// What the render thread knows about a frame when it finishes rendering.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameRecord {
    /// The take's frame ordinal, from zero.
    pub(crate) frame: u32,
    pub(crate) unix_ms: i64,
    pub(crate) game_run_time_us: Option<u64>,
    pub(crate) views: Option<CaptureViews>,
}

/// The take thread's handle, owned by the recorder state.
pub(crate) struct Take {
    sender: Option<Sender<Record>>,
    handle: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
    /// The render thread's end of a streamed take's video.
    video: Option<Arc<VideoChannel>>,
}

impl Take {
    /// Create the sidecar and spawn the thread, which connects to the host in the background.
    pub(crate) fn spawn(request: TakeRequest) -> anyhow::Result<Self> {
        let path = request.folder.join(RecordingFile::Frames.name());
        let sidecar = File::create(&path).with_context(|| {
            format!(
                "recorder: could not create the sidecar at {}",
                path.display()
            )
        })?;
        let (opening, writer) = if request.stream {
            let writer = Writer::spawn(&request.folder)?;
            let opening = Opening::Stream {
                take: request.stamp,
                host_folder: request.host_folder,
                payload_folder: request.folder.display().to_string(),
                window: request.window,
            };
            (opening, Some(writer))
        } else {
            let start = start_line(
                &request.stamp,
                request.host_folder.as_deref(),
                &request.window,
            );
            (Opening::Mirror(start), None)
        };
        let video = writer.as_ref().map(|writer| Arc::clone(writer.channel()));
        let target = request.target;
        let mut take = Self::spawn_thread(move |records, shared| {
            TakeThread {
                sidecar: BufWriter::new(sidecar),
                sidecar_failed: false,
                link: HostLink {
                    target,
                    opening,
                    writer,
                    state: LinkState::Waiting {
                        next_attempt: Instant::now(),
                    },
                },
                shared,
            }
            .run(records);
        })?;
        take.video = video;
        Ok(take)
    }

    fn spawn_thread(
        run: impl FnOnce(Receiver<Record>, Arc<Shared>) + Send + 'static,
    ) -> anyhow::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let shared = Arc::new(Shared {
            host: Mutex::new(HostState::Waiting),
            error: Mutex::new(None),
            lines: AtomicU32::new(0),
        });
        let thread_shared = Arc::clone(&shared);
        let handle = std::thread::Builder::new()
            .name("jc3vr-recorder".to_owned())
            .spawn(move || run(receiver, thread_shared))
            .context("recorder: could not spawn the take thread")?;
        Ok(Self {
            sender: Some(sender),
            handle: Some(handle),
            shared,
            video: None,
        })
    }

    /// Queue a sidecar line. Never blocks; hands the record back when the take is finishing or its
    /// thread has exited.
    pub(crate) fn send(&self, record: Record) -> Result<(), Record> {
        let Some(sender) = self.sender.as_ref() else {
            return Err(record);
        };
        sender
            .send(record)
            .map_err(|mpsc::SendError(record)| record)
    }

    /// The capture host's state.
    pub(crate) fn host(&self) -> HostState {
        self.shared.host.lock().clone()
    }

    /// The sidecar lines written so far.
    pub(crate) fn lines(&self) -> u32 {
        self.shared.lines.load(Ordering::Relaxed)
    }

    /// The take's error, if the host refused it, dropped it, or the sidecar failed.
    pub(crate) fn error(&self) -> Option<String> {
        self.shared.error.lock().clone()
    }

    /// A streamed take's video channel.
    pub(crate) fn video(&self) -> Option<&Arc<VideoChannel>> {
        self.video.as_ref()
    }

    /// Close the record channel and the video queue, so the thread flushes the sidecar, ends the
    /// stream, stops the host, and exits.
    pub(crate) fn finish(&mut self) {
        self.sender = None;
        if let Some(video) = &self.video {
            video.close();
        }
    }

    pub(super) fn is_finished(&self) -> bool {
        self.handle.as_ref().is_some_and(JoinHandle::is_finished)
    }

    /// A pending result keeps the join handle owned.
    pub(super) fn poll_completion(&mut self) -> Option<Completion> {
        if !self.is_finished() {
            return None;
        }
        let result = self
            .handle
            .take()
            .expect("recorder: a finished take has its handle")
            .join();
        let error = if result.is_err() {
            Some("recorder: the take thread panicked".to_owned())
        } else {
            self.shared.error.lock().clone()
        };
        Some(Completion { error })
    }

    #[cfg(test)]
    pub(super) fn spawn_controlled(
        run: impl FnOnce(Receiver<Record>) -> Result<(), String> + Send + 'static,
    ) -> Self {
        Self::spawn_thread(move |records, shared| {
            if let Err(error) = run(records) {
                *shared.error.lock() = Some(error);
            }
        })
        .expect("recorder: controlled take spawn")
    }
}

pub(super) struct Completion {
    pub(super) error: Option<String>,
}

/// How long the thread waits for a record before servicing the connection.
const POLL: Duration = Duration::from_millis(20);
/// How often the thread retries the host while waiting for the listener.
const CONNECT_RETRY: Duration = Duration::from_millis(500);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
/// How long the host has to confirm a take after the start line. The relay waits for the screen
/// recorder's first frame before confirming, which takes most of a second, and a streamed take's
/// host probes its encoders on the first stream of a size, which can take several seconds while a
/// hardware encoder's driver loads.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the thread waits for the host to confirm the video is finalized after the stop line.
const FINISH_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a streamed take's writer has to send what is queued and end the stream. It stops
/// writing after five seconds of its own.
const WRITER_FINISH_TIMEOUT: Duration = Duration::from_secs(8);
/// How long the thread waits for the host to finalize a streamed take's file after the stream
/// ends. The encoder drains its own queue of frames first.
const STREAM_FINISH_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a write the socket's buffer could not take waits before retrying. The socket is
/// non-blocking, since Winsock leaves a timed-out blocking send in an indeterminate state.
const WOULD_BLOCK_RETRY: Duration = Duration::from_millis(1);

struct Shared {
    host: Mutex<HostState>,
    error: Mutex<Option<String>>,
    lines: AtomicU32,
}

impl Shared {
    fn set_host(&self, state: HostState) {
        *self.host.lock() = state;
    }

    /// Record `message` as the take's error unless an earlier one explains it already.
    fn fail(&self, message: String) {
        tracing::error!("recorder: {message}");
        self.error.lock().get_or_insert(message);
    }
}

struct TakeThread {
    sidecar: BufWriter<File>,
    sidecar_failed: bool,
    link: HostLink,
    shared: Arc<Shared>,
}

impl TakeThread {
    fn run(mut self, records: Receiver<Record>) {
        loop {
            match records.recv_timeout(POLL) {
                Ok(record) => {
                    self.append(&record);
                    for record in records.try_iter() {
                        self.append(&record);
                    }
                    self.flush();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.link.poll(&self.shared);
        }
        self.flush();
        self.link.finish(&self.shared);
        tracing::info!(
            "recorder: take finished with {} sidecar lines",
            self.shared.lines.load(Ordering::Relaxed)
        );
    }

    fn append(&mut self, record: &Record) {
        if self.sidecar_failed {
            return;
        }
        match write_record(&mut self.sidecar, record) {
            Ok(()) => {
                self.shared.lines.fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => self.sidecar_error(error),
        }
    }

    fn flush(&mut self) {
        if !self.sidecar_failed
            && let Err(error) = self.sidecar.flush()
        {
            self.sidecar_error(error);
        }
    }

    fn sidecar_error(&mut self, error: std::io::Error) {
        self.sidecar_failed = true;
        self.shared
            .fail(format!("sidecar: could not write frames.jsonl: {error}"));
    }
}

/// Append `record` to the sidecar as one JSON line.
fn write_record(out: &mut impl Write, record: &Record) -> std::io::Result<()> {
    match record {
        Record::Frame(frame) => serde_json::to_writer(&mut *out, &FrameLine::new(frame)),
        Record::Marker { frame, marker } => serde_json::to_writer(
            &mut *out,
            &MarkerLine {
                frame: *frame,
                marker,
            },
        ),
        Record::Mirror {
            frame,
            unix_ms,
            mirror,
        } => serde_json::to_writer(
            &mut *out,
            &MirrorLine {
                frame: *frame,
                unix_ms: *unix_ms,
                mirror: MirrorJson {
                    view: mirror.view,
                    window: [mirror.window.0, mirror.window.1],
                    draws: mirror.draws.iter().flatten().collect(),
                },
            },
        ),
        Record::Present {
            frame,
            unix_ms,
            present,
            stamped,
        } => serde_json::to_writer(
            &mut *out,
            &PresentLine {
                frame: *frame,
                unix_ms: *unix_ms,
                present: *present,
                stamped: *stamped,
            },
        ),
    }
    .map_err(std::io::Error::other)?;
    out.write_all(b"\n")
}

#[derive(Serialize)]
struct FrameLine<'a> {
    frame: u32,
    unix_ms: i64,
    game_run_time_us: Option<u64>,
    predicted_display_time_ns: Option<i64>,
    views: Option<[ViewLine<'a>; 2]>,
}

impl<'a> FrameLine<'a> {
    fn new(frame: &'a FrameRecord) -> Self {
        Self {
            frame: frame.frame,
            unix_ms: frame.unix_ms,
            game_run_time_us: frame.game_run_time_us,
            predicted_display_time_ns: frame.views.map(|v| v.predicted_display_time_ns),
            views: frame
                .views
                .as_ref()
                .map(|v| [ViewLine::new(&v.eyes[0]), ViewLine::new(&v.eyes[1])]),
        }
    }
}

#[derive(Serialize)]
struct ViewLine<'a> {
    position: &'a [f32; 3],
    orientation: &'a [f32; 4],
    fov: &'a [f32; 4],
}

impl<'a> ViewLine<'a> {
    fn new(eye: &'a CaptureEyeView) -> Self {
        Self {
            position: &eye.position,
            orientation: &eye.orientation,
            fov: &eye.fov,
        }
    }
}

#[derive(Serialize)]
struct MarkerLine<'a> {
    frame: u32,
    marker: &'a Marker,
}

#[derive(Serialize)]
struct MirrorLine<'a> {
    frame: u32,
    unix_ms: i64,
    mirror: MirrorJson<'a>,
}

#[derive(Serialize)]
struct PresentLine {
    frame: u32,
    unix_ms: i64,
    present: u32,
    stamped: bool,
}

#[derive(Serialize)]
struct MirrorJson<'a> {
    view: MirrorView,
    window: [u32; 2],
    draws: Vec<&'a MirrorDraw>,
}

/// The connection to the capture host.
struct HostLink {
    target: String,
    opening: Opening,
    /// A streamed take's video writer.
    writer: Option<Writer>,
    state: LinkState,
}

/// What the take opens the connection with.
enum Opening {
    /// A mirror take's start line, ready to send.
    Mirror(Vec<u8>),
    /// A streamed take, whose start line waits for the stream's format.
    Stream {
        take: String,
        host_folder: Option<String>,
        payload_folder: String,
        window: WindowHint,
    },
}

enum LinkState {
    /// Not connected yet; the next attempt is due at `next_attempt`.
    Waiting { next_attempt: Instant },
    /// Connected and the start line sent. `deadline` bounds the wait for the host's confirmation.
    Open {
        stream: TcpStream,
        lines: LineBuffer,
        confirmed: bool,
        deadline: Instant,
    },
    /// The host refused or dropped the take; nothing more is sent.
    Closed,
}

impl HostLink {
    fn poll(&mut self, shared: &Shared) {
        self.state = match std::mem::replace(&mut self.state, LinkState::Closed) {
            LinkState::Waiting { next_attempt } if Instant::now() < next_attempt => {
                LinkState::Waiting { next_attempt }
            }
            LinkState::Waiting { next_attempt } => match self.start_bytes() {
                // A streamed take connects once the render thread has published the format.
                None => LinkState::Waiting { next_attempt },
                Some(start) => match self.connect(&start) {
                    Ok(stream) => {
                        tracing::info!(
                            "recorder: connected to the capture host at {}",
                            self.target
                        );
                        shared.set_host(HostState::Starting);
                        LinkState::Open {
                            stream,
                            lines: LineBuffer::default(),
                            confirmed: false,
                            deadline: Instant::now() + START_TIMEOUT,
                        }
                    }
                    Err(error) => {
                        tracing::debug!("recorder: waiting for the capture host: {error:#}");
                        LinkState::Waiting {
                            next_attempt: Instant::now() + CONNECT_RETRY,
                        }
                    }
                },
            },
            LinkState::Open {
                stream,
                lines,
                confirmed,
                deadline,
            } => self.poll_open(
                OpenLink {
                    stream,
                    lines,
                    confirmed,
                    deadline,
                },
                shared,
            ),
            LinkState::Closed => LinkState::Closed,
        };
    }

    fn poll_open(&mut self, mut link: OpenLink, shared: &Shared) -> LinkState {
        let (messages, closed) = match read_messages(&mut link.stream, &mut link.lines) {
            Ok(read) => read,
            Err(error) => {
                shared.fail(format!("capture host: {error}"));
                return self.close(shared);
            }
        };
        let mut open = true;
        for message in messages {
            let was_confirmed = link.confirmed;
            open &= handle_message(message, &mut link.confirmed, shared);
            if open && link.confirmed && !was_confirmed {
                open &= self.hand_stream(&link.stream, shared);
            }
        }
        if open && closed {
            shared.fail(if link.confirmed {
                "capture host: the host closed the connection during the take".to_owned()
            } else {
                "capture host: the host closed the connection before confirming the take".to_owned()
            });
            open = false;
        }
        if open && !link.confirmed && Instant::now() >= link.deadline {
            shared.fail(format!(
                "capture host: the host did not confirm the take within {} s",
                START_TIMEOUT.as_secs()
            ));
            open = false;
        }
        if open {
            LinkState::Open {
                stream: link.stream,
                lines: link.lines,
                confirmed: link.confirmed,
                deadline: link.deadline,
            }
        } else {
            self.close(shared)
        }
    }

    /// The start line, once it can be made.
    fn start_bytes(&self) -> Option<Vec<u8>> {
        match &self.opening {
            Opening::Mirror(start) => Some(start.clone()),
            Opening::Stream {
                take,
                host_folder,
                payload_folder,
                window,
            } => {
                let format = self.writer.as_ref()?.channel().format()?;
                Some(stream_start_line(
                    take,
                    host_folder.as_deref(),
                    payload_folder,
                    window,
                    format,
                ))
            }
        }
    }

    /// Give a streamed take's writer its own handle on the confirmed connection. Returns whether
    /// the connection stays open.
    fn hand_stream(&self, stream: &TcpStream, shared: &Shared) -> bool {
        let Some(writer) = &self.writer else {
            return true;
        };
        match stream.try_clone() {
            Ok(clone) => {
                writer.connect(clone);
                true
            }
            Err(error) => {
                shared.fail(format!(
                    "capture host: could not share the connection with the video writer: {error}"
                ));
                false
            }
        }
    }

    fn close(&self, shared: &Shared) -> LinkState {
        if let Some(writer) = &self.writer {
            writer.host_closed();
        }
        shared.set_host(HostState::Closed);
        LinkState::Closed
    }

    /// Connect to the host and send the start line.
    fn connect(&self, start: &[u8]) -> anyhow::Result<TcpStream> {
        let addresses: Vec<_> = self
            .target
            .to_socket_addrs()
            .with_context(|| format!("could not resolve {}", self.target))?
            .collect();
        let mut last_error = None;
        for address in &addresses {
            match TcpStream::connect_timeout(address, CONNECT_TIMEOUT) {
                Ok(mut stream) => {
                    let _ = stream.set_nodelay(true);
                    // A fresh socket's send buffer takes the whole start line, so this blocking
                    // write cannot stall.
                    stream
                        .write_all(start)
                        .context("could not send the start line")?;
                    stream.set_nonblocking(true)?;
                    return Ok(stream);
                }
                Err(error) => last_error = Some(error),
            }
        }
        match last_error {
            Some(error) => {
                Err(error).with_context(|| format!("could not connect to {}", self.target))
            }
            None => anyhow::bail!("{} resolves to no address", self.target),
        }
    }

    /// End the take: end the stream or send the stop line, and wait a bounded time for the host's
    /// confirmation.
    fn finish(&mut self, shared: &Shared) {
        let streamed = self.finish_writer(shared);
        match std::mem::replace(&mut self.state, LinkState::Closed) {
            LinkState::Waiting { .. } => {
                let no_format = self
                    .writer
                    .as_ref()
                    .is_some_and(|writer| writer.channel().format().is_none());
                shared.fail(if no_format {
                    "recorder: no eye views were captured, so the take has no video; the \
                     in-process capture needs a running VR session"
                        .to_owned()
                } else {
                    format!(
                        "capture host: no host listened at {}, so the take has no video",
                        self.target
                    )
                });
            }
            LinkState::Open {
                mut stream,
                mut lines,
                mut confirmed,
                ..
            } => {
                shared.set_host(HostState::Finishing);
                if self.writer.is_some() {
                    if !streamed {
                        shared.fail(
                            "recorder: no frame reached the capture host before the take ended"
                                .to_owned(),
                        );
                    }
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    await_stopped(
                        &mut stream,
                        &mut lines,
                        &mut confirmed,
                        STREAM_FINISH_TIMEOUT,
                        shared,
                    );
                } else {
                    let deadline = Instant::now() + FINISH_TIMEOUT;
                    if let Err(error) = write_nonblocking(&mut stream, &stop_line(), deadline) {
                        shared.fail(format!(
                            "capture host: could not send the stop line: {error}"
                        ));
                    } else {
                        await_stopped(
                            &mut stream,
                            &mut lines,
                            &mut confirmed,
                            FINISH_TIMEOUT,
                            shared,
                        );
                    }
                }
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
            LinkState::Closed => {}
        }
        shared.set_host(HostState::Closed);
    }

    /// Let a streamed take's writer send what is queued and end the stream. Returns whether any
    /// frame reached the host.
    fn finish_writer(&mut self, shared: &Shared) -> bool {
        let Some(writer) = self.writer.as_mut() else {
            return false;
        };
        if !writer.finish(WRITER_FINISH_TIMEOUT) {
            shared.fail(format!(
                "recorder: the video writer did not finish within {} s",
                WRITER_FINISH_TIMEOUT.as_secs()
            ));
        }
        if let Some(error) = writer.error() {
            shared.fail(error);
        }
        writer.wrote_frames()
    }
}

/// An open connection's state, moved through one poll.
struct OpenLink {
    stream: TcpStream,
    lines: LineBuffer,
    confirmed: bool,
    deadline: Instant,
}

/// Read until the host confirms the stop, closes, or `budget` passes.
fn await_stopped(
    stream: &mut TcpStream,
    lines: &mut LineBuffer,
    confirmed: &mut bool,
    budget: Duration,
    shared: &Shared,
) {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        let (messages, closed) = match read_messages(stream, lines) {
            Ok(read) => read,
            Err(error) => {
                shared.fail(format!("capture host: {error}"));
                return;
            }
        };
        for message in messages {
            match message {
                HostMessage::Stopped { exit_code } => {
                    match exit_code {
                        Some(0) => tracing::info!("recorder: the capture host finalized the video"),
                        Some(code) => shared.fail(format!(
                            "capture host: the recorder exited with status {code}"
                        )),
                        None => shared.fail(
                            "capture host: the recorder was killed before it finalized".to_owned(),
                        ),
                    }
                    return;
                }
                other => {
                    if !handle_message(other, confirmed, shared) {
                        return;
                    }
                }
            }
        }
        if closed {
            shared.fail(
                "capture host: the connection closed without confirming video finalization"
                    .to_owned(),
            );
            return;
        }
        std::thread::sleep(POLL);
    }
    shared.fail(format!(
        "capture host: video finalization was not confirmed within {} s",
        budget.as_secs_f64()
    ));
}

/// Apply one message from the host. Returns whether the connection stays open.
fn handle_message(message: HostMessage, confirmed: &mut bool, shared: &Shared) -> bool {
    match message {
        HostMessage::Started {
            file,
            video_start_unix_ms,
            region,
            output,
            encoder,
            audio,
        } => {
            match region {
                Some(region) => tracing::info!(
                    "recorder: the capture host records {}x{}+{}+{} on {} to {file}",
                    region.width,
                    region.height,
                    region.x,
                    region.y,
                    output.as_deref().unwrap_or("an unnamed output")
                ),
                None => tracing::info!(
                    "recorder: the capture host records the stream to {file} with {}; audio: {}",
                    encoder.as_deref().unwrap_or("an unnamed encoder"),
                    audio.as_deref().unwrap_or("not reported")
                ),
            }
            *confirmed = true;
            shared.set_host(HostState::Capturing(Capture {
                file,
                region,
                output,
                video_start_unix_ms,
                encoder,
                audio,
            }));
            true
        }
        HostMessage::Audio { state } => {
            tracing::info!("recorder: the capture host's audio: {state}");
            if let HostState::Capturing(capture) = &mut *shared.host.lock() {
                capture.audio = Some(state);
            }
            true
        }
        HostMessage::Error { message } => {
            shared.fail(message);
            false
        }
        HostMessage::Stopped { .. } | HostMessage::Unknown => true,
    }
}

/// The host's complete messages available without blocking, and whether the host has closed.
fn read_messages(
    stream: &mut TcpStream,
    lines: &mut LineBuffer,
) -> Result<(Vec<HostMessage>, bool), String> {
    let mut messages = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return Ok((messages, true)),
            Ok(count) => {
                for line in lines.feed(&buffer[..count]).map_err(|e| e.to_string())? {
                    messages.push(parse_host_line(&line).map_err(|e| e.to_string())?);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Ok((messages, false));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("the connection failed: {error}")),
        }
    }
}

fn write_nonblocking(
    stream: &mut impl Write,
    mut bytes: &[u8],
    deadline: Instant,
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= deadline {
            return Err(std::io::ErrorKind::TimedOut.into());
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
