//! The tracking recorder's bounded writer queue and nonblocking completion reporting.

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::JoinHandle,
};

use parking_lot::Mutex;
use tracking_format::input::{CapturedInput, DiscontinuityReason, InputRecord};

use crate::recorder::tracking::format::{self, Header};

/// The container selected once at recording start. Compression remains an explicit experiment
/// until representative full-joint capture and sustained-I/O acceptance pass.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub(crate) enum RecordingFormat {
    #[default]
    UncompressedV2,
    ChunkedV3,
}

impl RecordingFormat {
    pub(crate) fn from_compression_opt_in(enabled: bool) -> Self {
        if enabled {
            Self::ChunkedV3
        } else {
            Self::UncompressedV2
        }
    }

    fn preamble(self, header: &Header) -> Vec<u8> {
        match self {
            Self::UncompressedV2 => format::encode_input_preamble(header),
            Self::ChunkedV3 => format::encode_chunked_preamble(header),
        }
    }
}

/// A frame and all input records captured beside it occupy one queue slot.
pub(crate) struct CaptureBatch {
    pub(crate) bytes: Vec<u8>,
    pub(crate) capture_time_ns: u64,
    pub(crate) input_groups: u64,
}

#[derive(Debug)]
pub(crate) enum WriterError {
    Create {
        path: PathBuf,
        error: std::io::Error,
    },
    Spawn(std::io::Error),
}

impl std::fmt::Display for WriterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriterError::Create { path, error } => {
                write!(f, "writer: could not create {}: {error}", path.display())
            }
            WriterError::Spawn(error) => write!(f, "writer: could not spawn the thread: {error}"),
        }
    }
}

impl std::error::Error for WriterError {}

pub(crate) enum SendOutcome {
    Queued,
    Full,
    Failed(String),
}

/// Completion proves the thread has exited. An I/O error or panic does not make unmapping unsafe.
#[derive(Debug)]
pub(crate) struct Completion {
    pub(crate) bytes: u64,
    pub(crate) error: Option<String>,
}

/// The owner retains the join handle until a nonblocking poll confirms thread termination.
pub(crate) struct Writer {
    sender: Option<SyncSender<Vec<u8>>>,
    handle: Option<Owner>,
    shared: Arc<Shared>,
    pending_loss: Option<Loss>,
}

#[cfg(test)]
pub(super) type ControlledCompletion = Arc<Mutex<Option<Completion>>>;

enum Owner {
    Thread(JoinHandle<Result<(), String>>),
    #[cfg(test)]
    Controlled(ControlledCompletion),
}

impl Owner {
    fn is_finished(&self) -> bool {
        match self {
            Self::Thread(handle) => handle.is_finished(),
            #[cfg(test)]
            Self::Controlled(result) => result.lock().is_some(),
        }
    }
}

impl Writer {
    pub(crate) fn start(
        path: &Path,
        header: &Header,
        format: RecordingFormat,
    ) -> Result<Self, WriterError> {
        let preamble = format.preamble(header);
        let create = || -> std::io::Result<BufWriter<File>> {
            let mut file = BufWriter::new(File::create_new(path)?);
            file.write_all(&preamble)?;
            Ok(file)
        };
        let file = create().map_err(|error| WriterError::Create {
            path: path.to_owned(),
            error,
        })?;
        Self::spawn_sink(file, preamble.len() as u64, format)
    }

    fn spawn_sink(
        file: impl Sink + Send + 'static,
        bytes: u64,
        format: RecordingFormat,
    ) -> Result<Self, WriterError> {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_DEPTH);
        let shared = Arc::new(Shared::new(bytes, format));
        let thread_shared = Arc::clone(&shared);
        let handle = std::thread::Builder::new()
            .name("jc3vr-tracking-writer".to_owned())
            .spawn(move || run(file, &receiver, &thread_shared))
            .map_err(WriterError::Spawn)?;
        Ok(Self {
            sender: Some(sender),
            handle: Some(Owner::Thread(handle)),
            shared,
            pending_loss: None,
        })
    }

    /// Queue an atomic capture without blocking. Loss precedes the next accepted batch or is
    /// written after the accepted queue drains when recording stops first.
    pub(crate) fn send(&mut self, mut batch: CaptureBatch) -> SendOutcome {
        if batch.bytes.len() > MAX_BATCH_BYTES {
            return SendOutcome::Failed(format!(
                "tracking capture batch exceeds the {MAX_BATCH_BYTES}-byte bound"
            ));
        }
        if let Some(loss) = self.pending_loss {
            let mut prefix = encode_loss(loss);
            prefix.append(&mut batch.bytes);
            batch.bytes = prefix;
        }
        let outcome = self.send_bytes(batch.bytes);
        match outcome {
            SendOutcome::Queued => self.pending_loss = None,
            SendOutcome::Full => {
                let loss = self.pending_loss.get_or_insert(Loss {
                    time_ns: batch.capture_time_ns,
                    groups: 0,
                });
                loss.groups = loss.groups.saturating_add(batch.input_groups);
            }
            SendOutcome::Failed(_) => {}
        }
        outcome
    }

    fn send_bytes(&self, record: Vec<u8>) -> SendOutcome {
        if let Some(error) = self.error() {
            return SendOutcome::Failed(error);
        }
        let Some(sender) = self.sender.as_ref() else {
            return SendOutcome::Failed("the writer is finishing".to_owned());
        };
        match sender.try_send(record) {
            Ok(()) => SendOutcome::Queued,
            Err(TrySendError::Full(_)) => SendOutcome::Full,
            Err(TrySendError::Disconnected(_)) => {
                SendOutcome::Failed("the writer thread has exited".to_owned())
            }
        }
    }

    /// Encoded bytes accepted by the buffered sink, including the preamble. A flush error can
    /// leave fewer durable bytes on disk.
    pub(crate) fn bytes_written(&self) -> u64 {
        self.shared.bytes.load(Ordering::Relaxed)
    }

    /// Preamble bytes plus original record bytes in fully accepted batches. Partial writes
    /// contribute stored bytes only; neither counter claims durability before flush and sync.
    pub(crate) fn logical_bytes_written(&self) -> u64 {
        self.shared.logical_bytes.load(Ordering::Relaxed)
    }

    pub(crate) fn error(&self) -> Option<String> {
        self.shared.error.lock().clone()
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.handle.as_ref().is_some_and(Owner::is_finished)
    }

    pub(crate) fn finish(&mut self) {
        if let Some(loss) = self.pending_loss.take() {
            *self.shared.final_loss.lock() = Some(loss);
        }
        self.sender = None;
    }

    /// Never waits for a running thread and never consumes its owner on a pending result.
    pub(crate) fn poll_completion(&mut self) -> Option<Completion> {
        if !self.is_finished() {
            return None;
        }
        match self
            .handle
            .take()
            .expect("tracking writer: finished handle")
        {
            Owner::Thread(handle) => Some(completion(handle.join(), self.bytes_written())),
            #[cfg(test)]
            Owner::Controlled(result) => result.lock().take(),
        }
    }

    #[cfg(test)]
    pub(super) fn controlled() -> (Self, ControlledCompletion) {
        let result = Arc::new(Mutex::new(None));
        (
            Self {
                sender: None,
                handle: Some(Owner::Controlled(Arc::clone(&result))),
                shared: Arc::new(Shared::new(0, RecordingFormat::default())),
                pending_loss: None,
            },
            result,
        )
    }
}

fn completion(result: std::thread::Result<Result<(), String>>, bytes: u64) -> Completion {
    let error = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some("tracking writer: the writer thread panicked".to_owned()),
    };
    Completion { bytes, error }
}

const QUEUE_DEPTH: usize = 32;
pub(super) const MAX_BATCH_BYTES: usize = 2 * 1024 * 1024;

#[derive(Copy, Clone)]
struct Loss {
    time_ns: u64,
    groups: u64,
}

fn encode_loss(loss: Loss) -> Vec<u8> {
    tracking_format::encode_input(&CapturedInput {
        capture_time_ns: loss.time_ns,
        frame_sequence: None,
        record: InputRecord::Discontinuity {
            reason: DiscontinuityReason::Overflow,
            discarded: loss.groups,
        },
    })
    .expect("tracking writer: fixed loss record is bounded")
}

struct Shared {
    format: RecordingFormat,
    error: Mutex<Option<String>>,
    bytes: AtomicU64,
    logical_bytes: AtomicU64,
    final_loss: Mutex<Option<Loss>>,
}

impl Shared {
    fn new(bytes: u64, format: RecordingFormat) -> Self {
        Self {
            format,
            error: Mutex::new(None),
            bytes: AtomicU64::new(bytes),
            logical_bytes: AtomicU64::new(bytes),
            final_loss: Mutex::new(None),
        }
    }
}

trait Sink: Write {
    fn sync(&mut self) -> std::io::Result<()>;
}

impl Sink for BufWriter<File> {
    fn sync(&mut self) -> std::io::Result<()> {
        self.get_ref().sync_all()
    }
}

fn run(mut file: impl Sink, receiver: &Receiver<Vec<u8>>, shared: &Shared) -> Result<(), String> {
    let result = drain(&mut file, receiver, shared);
    if let Err(message) = &result {
        tracing::error!("recorder: tracking: writer: {message}");
        *shared.error.lock() = Some(message.clone());
    }
    result
}

fn drain(
    file: &mut impl Sink,
    receiver: &Receiver<Vec<u8>>,
    shared: &Shared,
) -> Result<(), String> {
    for record in receiver {
        write_batch(file, &record, shared)
            .map_err(|error| format!("tracking writer: record write: {error}"))?;
    }
    if let Some(loss) = shared.final_loss.lock().take() {
        write_batch(file, &encode_loss(loss), shared)
            .map_err(|error| format!("tracking writer: final input loss write: {error}"))?;
    }
    file.flush()
        .map_err(|e| format!("tracking writer: flush: {e}"))?;
    file.sync()
        .map_err(|e| format!("tracking writer: sync: {e}"))
}

fn write_batch(file: &mut impl Sink, records: &[u8], shared: &Shared) -> Result<(), String> {
    let chunk;
    let stored = match shared.format {
        RecordingFormat::UncompressedV2 => records,
        RecordingFormat::ChunkedV3 => {
            chunk = tracking_format::encode_chunk(records).map_err(|error| error.to_string())?;
            chunk.as_slice()
        }
    };
    write_record(file, stored, shared).map_err(|error| error.to_string())?;
    shared
        .logical_bytes
        .fetch_add(records.len() as u64, Ordering::Relaxed);
    Ok(())
}

fn write_record(file: &mut impl Sink, mut bytes: &[u8], shared: &Shared) -> std::io::Result<()> {
    while !bytes.is_empty() {
        match file.write(bytes) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(count) => {
                shared.bytes.fetch_add(count as u64, Ordering::Relaxed);
                bytes = &bytes[count..];
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
