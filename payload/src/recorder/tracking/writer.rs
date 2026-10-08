//! The tracking recorder's writer thread: appends the encoded records to the recording's file, so
//! the game thread never waits on the disk.

use std::{
    fs::File,
    io::{BufWriter, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

/// Why the writer could not be started.
#[derive(Debug)]
pub(crate) enum WriterError {
    /// The recording's file could not be created or its preamble written.
    Create {
        path: PathBuf,
        error: std::io::Error,
    },
    /// The thread could not be spawned.
    Spawn(std::io::Error),
}

impl std::fmt::Display for WriterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriterError::Create { path, error } => {
                write!(f, "could not create {}: {error}", path.display())
            }
            WriterError::Spawn(error) => write!(f, "could not spawn the writer thread: {error}"),
        }
    }
}

impl std::error::Error for WriterError {}

/// The result of queueing a record.
pub(crate) enum SendOutcome {
    Queued,
    /// The queue is full; the record is dropped.
    Full,
    /// The writer has failed with this message; the recording should stop.
    Failed(String),
}

/// The writer thread's handle.
pub(crate) struct Writer {
    sender: Option<SyncSender<Vec<u8>>>,
    handle: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
}

impl Writer {
    /// Create the file at `path`, write `preamble` to it, and spawn the thread that appends the
    /// records sent to it.
    pub(crate) fn spawn(path: &Path, preamble: &[u8]) -> Result<Self, WriterError> {
        let create = |path: &Path| -> std::io::Result<BufWriter<File>> {
            let mut file = BufWriter::new(File::create_new(path)?);
            file.write_all(preamble)?;
            Ok(file)
        };
        let file = create(path).map_err(|error| WriterError::Create {
            path: path.to_owned(),
            error,
        })?;
        let (sender, receiver) = mpsc::sync_channel(QUEUE_DEPTH);
        let shared = Arc::new(Shared {
            error: Mutex::new(None),
            bytes: AtomicU64::new(preamble.len() as u64),
        });
        let thread_shared = Arc::clone(&shared);
        let handle = std::thread::Builder::new()
            .name("jc3vr-tracking-writer".to_owned())
            .spawn(move || run(file, &receiver, &thread_shared))
            .map_err(WriterError::Spawn)?;
        Ok(Self {
            sender: Some(sender),
            handle: Some(handle),
            shared,
        })
    }

    /// Queue a record without blocking.
    pub(crate) fn send(&self, record: Vec<u8>) -> SendOutcome {
        if let Some(error) = self.shared.error.lock().clone() {
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

    /// How many bytes the file holds so far.
    pub(crate) fn bytes_written(&self) -> u64 {
        self.shared.bytes.load(Ordering::Relaxed)
    }

    /// The error the writer stopped on, if any.
    pub(crate) fn error(&self) -> Option<String> {
        self.shared.error.lock().clone()
    }

    /// Close the queue: the thread writes what it has, flushes, and exits.
    pub(crate) fn finish(&mut self) {
        self.sender = None;
    }

    /// Wait for the thread to exit, up to `budget`. Returns whether it did.
    pub(crate) fn join(mut self, budget: Duration) -> bool {
        self.finish();
        let Some(handle) = self.handle.take() else {
            return true;
        };
        let deadline = Instant::now() + budget;
        while !handle.is_finished() {
            if Instant::now() >= deadline {
                tracing::error!(
                    "recorder: tracking: the writer did not finish within {} s; leaving the \
                     payload mapped rather than unmapping under a live thread",
                    budget.as_secs()
                );
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = handle.join();
        true
    }
}

/// How many records may wait for the writer before the game thread drops them. A record is about
/// a kilobyte, three with the hand joints, so this is under a megabyte and a few seconds.
const QUEUE_DEPTH: usize = 512;

struct Shared {
    error: Mutex<Option<String>>,
    bytes: AtomicU64,
}

fn run(mut file: BufWriter<File>, receiver: &Receiver<Vec<u8>>, shared: &Shared) {
    for record in receiver {
        if let Err(e) = file.write_all(&record) {
            fail(shared, &format!("could not write a record: {e}"));
            return;
        }
        shared
            .bytes
            .fetch_add(record.len() as u64, Ordering::Relaxed);
    }
    if let Err(e) = file.flush().and_then(|()| file.get_ref().sync_all()) {
        fail(shared, &format!("could not flush the recording: {e}"));
    }
}

fn fail(shared: &Shared, message: &str) {
    tracing::error!("recorder: tracking: writer: {message}");
    *shared.error.lock() = Some(message.to_owned());
}
