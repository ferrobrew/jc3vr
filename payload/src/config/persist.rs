//! Loading the settings file at startup and saving it as the user changes settings.
//!
//! The save has explicit write points: [`crate::config::edit`] marks the configuration changed
//! ([`note_edit`]), and the game thread's per-frame [`tick`] saves once the changes settle, at
//! most [`MAX_DELAY`] after the first unsaved one so that a control held through every frame (an
//! open debug UI tab edits each frame) still saves. A save serializes a snapshot and compares it
//! with the text the file was last given; only a difference is written, by a short-lived writer
//! thread, so the frame never waits on the disk. Writes are serialized and numbered: a writer that
//! finds a newer save already written skips its own. [`shutdown`] flushes a pending save.
//!
//! Locks: the saver state is taken outermost, then the configuration lock, then the edit marks and
//! the automatic overrides, which are leaves. Nothing takes the saver state with the configuration
//! locked.

use std::{
    fmt, fs, io, mem,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use crate::config::{
    CONFIG, Config, automatic, load, paths,
    write::{self, WriteError},
};

/// The state of the settings file, for a readout.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SaveStatus {
    /// The file holds the settings, or nothing has changed since they were loaded.
    UpToDate,
    /// A change is waiting to settle or being written.
    Pending,
    /// The last save failed; the log says why. The next change tries again.
    Failed,
    /// There is no configuration directory, so nothing is saved.
    Unavailable,
}

/// Load the settings file into the configuration. Called once at startup, before anything reads
/// the configuration for initialization. Every failure falls back to the defaults: a missing file
/// is the first run, and an unreadable or unparseable one is left as it is until a setting changes.
pub(crate) fn init() {
    let Some(path) = paths::settings_file() else {
        return;
    };
    tracing::info!(path = %path.display(), "settings: using this settings file");
    let config = load_from(&path);
    // The baseline the first save compares against: what the configuration was loaded as, so that
    // nothing is written (and a broken file is not replaced) until a setting changes.
    let baseline = toml::to_string(&config)
        .inspect_err(|error| {
            tracing::warn!("settings: could not serialize the loaded settings: {error}")
        })
        .ok();
    *CONFIG.lock() = config;
    SAVER.lock().last_text = baseline;
}

/// Mark the configuration changed by the user. Called by the edit guard with the configuration
/// lock held.
pub(super) fn note_edit() {
    EDITS.lock().note(Instant::now());
}

/// Save the configuration if the user's changes have settled. Called once per frame on the game
/// thread; the disk write itself runs on a writer thread.
pub(crate) fn tick() {
    let mut saver = SAVER.lock();
    saver.reap();
    if saver.closed || !EDITS.lock().take_if_due(Instant::now()) {
        return;
    }
    let Some(text) = serialize_snapshot(&mut saver) else {
        return;
    };
    saver.hand_off(text);
}

/// Stop saving, wait for the writers, and write a pending change. Runs on the shutdown thread.
/// Returns `false` when a writer did not finish in time, in which case the payload must stay
/// mapped.
#[must_use = "a writer that did not finish means the payload must stay mapped"]
pub(crate) fn shutdown() -> bool {
    let writers = {
        let mut saver = SAVER.lock();
        saver.closed = true;
        mem::take(&mut saver.writers)
    };
    let deadline = Instant::now() + SHUTDOWN_BUDGET;
    for writer in writers {
        while !writer.handle.is_finished() {
            if Instant::now() >= deadline {
                tracing::error!(
                    "settings: a writer outlived the shutdown deadline; keeping the payload mapped"
                );
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let mut saver = SAVER.lock();
        saver.settle(writer);
    }
    let mut saver = SAVER.lock();
    if !EDITS.lock().take() {
        return true;
    }
    let Some(text) = serialize_snapshot(&mut saver) else {
        return true;
    };
    if saver.last_text.as_deref() == Some(text.as_str()) {
        return true;
    }
    let Some(path) = paths::settings_file() else {
        return true;
    };
    saver.generation += 1;
    let generation = saver.generation;
    drop(saver);
    match write_generation(generation, &path, &text) {
        Ok(_) => tracing::info!(path = %path.display(), "settings: saved the last changes"),
        Err(error) => tracing::warn!("{error}"),
    }
    true
}

/// The settings file's state.
pub(crate) fn status() -> SaveStatus {
    let saver = SAVER.lock();
    if paths::dir().is_none() {
        SaveStatus::Unavailable
    } else if !saver.writers.is_empty() || EDITS.lock().pending() {
        SaveStatus::Pending
    } else if saver.failed {
        SaveStatus::Failed
    } else {
        SaveStatus::UpToDate
    }
}

/// How long the changes must be quiet before a save.
const QUIET: Duration = Duration::from_secs(1);

/// The longest a change waits for the changes after it to settle.
const MAX_DELAY: Duration = Duration::from_secs(5);

/// How long shutdown waits for the writers.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(10);

static SAVER: Mutex<Saver> = Mutex::new(Saver::new());

static EDITS: Mutex<Edits> = Mutex::new(Edits::new());

/// The generation the file holds. Held for each whole write, which serializes the writers.
static WRITTEN: Mutex<u64> = Mutex::new(0);

/// The unsaved changes: when the first and the last of them were made.
struct Edits {
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Edits {
    const fn new() -> Self {
        Self {
            first: None,
            last: None,
        }
    }

    fn note(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    fn pending(&self) -> bool {
        self.first.is_some()
    }

    /// Clear the changes and return whether there were any.
    fn take(&mut self) -> bool {
        mem::replace(self, Self::new()).pending()
    }

    /// Clear the changes and return `true` when a save is due at `now`.
    fn take_if_due(&mut self, now: Instant) -> bool {
        let due = match (self.first, self.last) {
            (Some(first), Some(last)) => {
                now.duration_since(last) >= QUIET || now.duration_since(first) >= MAX_DELAY
            }
            _ => false,
        };
        due && self.take()
    }
}

struct Saver {
    /// Shutdown has begun; nothing more is handed to a writer.
    closed: bool,
    /// The text the file was last given, or loaded as. A save of the same text is skipped.
    last_text: Option<String>,
    /// The number of the latest save handed to a writer.
    generation: u64,
    writers: Vec<Writer>,
    /// The latest save failed.
    failed: bool,
}

struct Writer {
    generation: u64,
    handle: JoinHandle<Result<Written, SaveError>>,
}

/// What a writer did.
enum Written {
    /// It wrote its text.
    Written,
    /// A newer save had already been written, so it wrote nothing.
    Superseded,
}

impl Saver {
    const fn new() -> Self {
        Self {
            closed: false,
            last_text: None,
            generation: 0,
            writers: Vec::new(),
            failed: false,
        }
    }

    /// Hand `text` to a writer thread, unless the file already holds it.
    fn hand_off(&mut self, text: String) {
        if self.last_text.as_deref() == Some(text.as_str()) {
            return;
        }
        let Some(path) = paths::settings_file() else {
            return;
        };
        self.generation += 1;
        let generation = self.generation;
        let written = text.clone();
        match thread::Builder::new()
            .name("jc3vr-settings-writer".into())
            .spawn(move || write_generation(generation, &path, &written))
        {
            Ok(handle) => {
                self.last_text = Some(text);
                self.writers.push(Writer { generation, handle });
            }
            Err(source) => {
                self.failed = true;
                tracing::warn!("{}", SaveError::Spawn(source));
            }
        }
    }

    /// Account for the writers that have finished.
    fn reap(&mut self) {
        if self
            .writers
            .iter()
            .all(|writer| !writer.handle.is_finished())
        {
            return;
        }
        let (finished, running) = mem::take(&mut self.writers)
            .into_iter()
            .partition(|writer| writer.handle.is_finished());
        self.writers = running;
        for writer in finished {
            self.settle(writer);
        }
    }

    /// Record a finished writer's outcome. A failure of the latest save forgets the text it was
    /// to write, so that the next change writes again even if it restores that text.
    fn settle(&mut self, writer: Writer) {
        let latest = writer.generation == self.generation;
        match writer.handle.join() {
            Ok(Ok(Written::Written)) => {
                tracing::debug!("settings: saved");
                if latest {
                    self.failed = false;
                }
            }
            Ok(Ok(Written::Superseded)) => {}
            Ok(Err(error)) => {
                tracing::warn!("{error}");
                if latest {
                    self.failed = true;
                    self.last_text = None;
                }
            }
            Err(_) => {
                tracing::warn!("{}", SaveError::WriterPanicked);
                if latest {
                    self.failed = true;
                    self.last_text = None;
                }
            }
        }
    }
}

/// Why a save failed.
#[derive(Debug)]
enum SaveError {
    /// The configuration could not be serialized.
    Serialize(toml::ser::Error),
    /// The writer thread could not be started.
    Spawn(io::Error),
    /// The writer thread panicked.
    WriterPanicked,
    /// The file could not be written.
    Write(WriteError),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "settings: ")?;
        match self {
            Self::Serialize(source) => write!(f, "could not serialize the settings: {source}"),
            Self::Spawn(source) => write!(f, "could not start the writer: {source}"),
            Self::WriterPanicked => write!(f, "the writer panicked"),
            Self::Write(source) => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialize(source) => Some(source),
            Self::Spawn(source) => Some(source),
            Self::WriterPanicked => None,
            Self::Write(source) => Some(source),
        }
    }
}

/// Why the settings file could not be read.
#[derive(Debug)]
struct ReadError {
    path: PathBuf,
    source: io::Error,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "settings: could not read {}: {}",
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Read and load the settings file at `path`, falling back to the defaults and logging why.
fn load_from(path: &Path) -> Config {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            tracing::info!("settings: no settings file yet; using the defaults");
            return Config::new();
        }
        Err(source) => {
            let error = ReadError {
                path: path.to_owned(),
                source,
            };
            tracing::warn!(
                "{error}; using the defaults, and leaving the file alone until a setting changes"
            );
            return Config::new();
        }
    };
    match load::parse(&text) {
        Ok(loaded) => {
            loaded.report.log(&path.display().to_string());
            if loaded.report.is_empty() {
                tracing::info!("settings: loaded");
            }
            loaded.config
        }
        Err(error) => {
            tracing::warn!(
                "{error}; using the defaults, and leaving the file alone until a setting changes"
            );
            Config::new()
        }
    }
}

/// Serialize the configuration as a save writes it: with the user's values for the switches the
/// payload turned off on its own. Takes the saver state so that the lock order is explicit.
fn serialize_snapshot(saver: &mut Saver) -> Option<String> {
    let snapshot = {
        let config = CONFIG.lock();
        let mut snapshot = config.clone();
        automatic::apply(&mut snapshot);
        snapshot
    };
    match toml::to_string(&snapshot) {
        Ok(text) => Some(text),
        Err(source) => {
            saver.failed = true;
            tracing::warn!("{}", SaveError::Serialize(source));
            None
        }
    }
}

/// Write `text` as save number `generation`, unless a newer save is already written.
fn write_generation(generation: u64, path: &Path, text: &str) -> Result<Written, SaveError> {
    let mut written = WRITTEN.lock();
    if *written > generation {
        return Ok(Written::Superseded);
    }
    write::replace(path, text.as_bytes()).map_err(SaveError::Write)?;
    *written = generation;
    Ok(Written::Written)
}
