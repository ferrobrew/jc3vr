//! Loading the settings file at startup and saving it as the user changes settings.
//!
//! The save has explicit write points: [`crate::config::edit`] marks the configuration changed
//! ([`note_edit`]), and the game thread's per-frame [`tick`] saves once the changes settle, at
//! most [`MAX_DELAY`] after the first unsaved one so that a control held through every frame (an
//! open debug UI tab edits each frame) still saves. A save serializes a snapshot and compares it
//! with the text the file was last given; only a difference is written, by a short-lived writer
//! thread, so the frame never waits on the disk. Writes are serialized and numbered: a writer that
//! finds a newer save already written skips its own. A failed save is retried after a backoff
//! that grows with each failure, without waiting for another change. [`shutdown`] flushes a
//! pending save, and one that failed.
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
    /// The last save failed; the log says why. It is retried after a backoff.
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
    let now = Instant::now();
    let mut saver = SAVER.lock();
    saver.reap(now);
    if saver.closed {
        return;
    }
    let (pending, due) = {
        let mut edits = EDITS.lock();
        (edits.pending(), edits.take_if_due(now))
    };
    let retry = saver.retry.take_if_due(now);
    if !due && !retry && !(pending && saver.comparison_due(now)) {
        return;
    }
    let Some(text) = serialize_snapshot(&mut saver) else {
        return;
    };
    // A change that has not settled is compared with the file now and then, so that edits which
    // leave the settings as the file has them (an open debug UI tab marks one every frame) read as
    // up to date.
    saver.compared = Some(now);
    saver.unchanged = saver.last_text.as_deref() == Some(text.as_str());
    if due || retry {
        saver.hand_off(text, now);
    }
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
        saver.settle(writer, Instant::now());
    }
    let mut saver = SAVER.lock();
    let edited = EDITS.lock().take();
    // A failed save forgot the text the file holds, so it is written here even with no change
    // since, rather than leaving the file behind the settings until the next launch.
    if !edited && !saver.failed && saver.last_text.is_some() {
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
    status_of(StatusInputs {
        available: paths::dir().is_some(),
        writing: !saver.writers.is_empty(),
        failed: saver.failed,
        changed: EDITS.lock().pending() && !saver.unchanged,
    })
}

/// How long the changes must be quiet before a save.
const QUIET: Duration = Duration::from_secs(1);

/// The longest a change waits for the changes after it to settle.
const MAX_DELAY: Duration = Duration::from_secs(5);

/// How long the first retry of a failed save waits. Each further failure doubles it, up to
/// [`RETRY_MAX`].
const RETRY_FIRST: Duration = Duration::from_secs(2);

/// The longest a retry of a failed save waits.
const RETRY_MAX: Duration = Duration::from_secs(60);

/// How often a change that has not settled is compared with the file, for [`status`].
const COMPARE_INTERVAL: Duration = Duration::from_secs(1);

/// How long shutdown waits for the writers.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(10);

static SAVER: Mutex<Saver> = Mutex::new(Saver::new());

static EDITS: Mutex<Edits> = Mutex::new(Edits::new());

/// The generation the file holds. Held for each whole write, which serializes the writers.
static WRITTEN: Mutex<u64> = Mutex::new(0);

/// What [`status`] reads.
#[derive(Copy, Clone, Debug)]
struct StatusInputs {
    /// There is a configuration directory.
    available: bool,
    /// A writer is running.
    writing: bool,
    /// The latest save failed.
    failed: bool,
    /// A change is waiting to settle, and the settings it left differ from the file's text (or
    /// have not been compared with it yet).
    changed: bool,
}

/// The file's state from what [`status`] reads. A failure outranks a waiting change, so that a
/// failed save still reads as failed while a debug UI tab marks a change every frame.
fn status_of(inputs: StatusInputs) -> SaveStatus {
    if !inputs.available {
        SaveStatus::Unavailable
    } else if inputs.writing {
        SaveStatus::Pending
    } else if inputs.failed {
        SaveStatus::Failed
    } else if inputs.changed {
        SaveStatus::Pending
    } else {
        SaveStatus::UpToDate
    }
}

/// When a failed save is next tried, with a delay that doubles with each failure.
#[derive(Debug)]
struct Retry {
    at: Option<Instant>,
    delay: Duration,
}

impl Retry {
    const fn new() -> Self {
        Self {
            at: None,
            delay: RETRY_FIRST,
        }
    }

    /// Schedule a retry after a failure at `now`.
    fn fail(&mut self, now: Instant) {
        self.at = Some(now + self.delay);
        self.delay = (self.delay * 2).min(RETRY_MAX);
    }

    /// Forget the failures after a save succeeds.
    fn succeed(&mut self) {
        *self = Self::new();
    }

    /// Clear the scheduled retry and return `true` when it is due at `now`.
    fn take_if_due(&mut self, now: Instant) -> bool {
        match self.at {
            Some(at) if now >= at => {
                self.at = None;
                true
            }
            _ => false,
        }
    }
}

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
    /// When a failed save is tried again.
    retry: Retry,
    /// When the settings were last compared with the file's text.
    compared: Option<Instant>,
    /// The last comparison found the settings as the file has them.
    unchanged: bool,
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
            retry: Retry::new(),
            compared: None,
            unchanged: false,
        }
    }

    /// Whether a change that has not settled is due a comparison with the file at `now`.
    fn comparison_due(&self, now: Instant) -> bool {
        self.compared
            .is_none_or(|compared| now.duration_since(compared) >= COMPARE_INTERVAL)
    }

    /// Hand `text` to a writer thread, unless the file already holds it.
    fn hand_off(&mut self, text: String, now: Instant) {
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
                self.unchanged = true;
                self.writers.push(Writer { generation, handle });
            }
            Err(source) => {
                self.failed = true;
                self.retry.fail(now);
                tracing::warn!("{}", SaveError::Spawn(source));
            }
        }
    }

    /// Account for the writers that have finished.
    fn reap(&mut self, now: Instant) {
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
            self.settle(writer, now);
        }
    }

    /// Record a finished writer's outcome. A failure of the latest save forgets the text it was
    /// to write, so that its retry (and any later save) writes even if the settings are back to
    /// that text, and schedules the retry.
    fn settle(&mut self, writer: Writer, now: Instant) {
        let latest = writer.generation == self.generation;
        match writer.handle.join() {
            Ok(Ok(Written::Written)) => {
                tracing::debug!("settings: saved");
                if latest {
                    self.failed = false;
                    self.retry.succeed();
                }
            }
            Ok(Ok(Written::Superseded)) => {}
            Ok(Err(error)) => {
                tracing::warn!("{error}");
                if latest {
                    self.fail_latest(now);
                }
            }
            Err(_) => {
                tracing::warn!("{}", SaveError::WriterPanicked);
                if latest {
                    self.fail_latest(now);
                }
            }
        }
    }

    fn fail_latest(&mut self, now: Instant) {
        self.failed = true;
        self.last_text = None;
        self.unchanged = false;
        self.retry.fail(now);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retry_waits_and_backs_off_up_to_the_cap() {
        let start = Instant::now();
        let mut retry = Retry::new();
        assert!(!retry.take_if_due(start + RETRY_MAX * 10));

        retry.fail(start);
        assert!(!retry.take_if_due(start + RETRY_FIRST / 2));
        assert!(retry.take_if_due(start + RETRY_FIRST));
        assert!(
            !retry.take_if_due(start + RETRY_FIRST),
            "a retry is taken once"
        );

        let mut now = start;
        let mut last_delay = Duration::ZERO;
        for _ in 0..16 {
            retry.fail(now);
            let at = retry.at.expect("a failure schedules a retry");
            let delay = at - now;
            assert!(delay >= last_delay && delay <= RETRY_MAX);
            last_delay = delay;
            now = at;
            assert!(retry.take_if_due(now));
        }
        assert_eq!(last_delay, RETRY_MAX);

        retry.succeed();
        retry.fail(now);
        assert_eq!(
            retry.at,
            Some(now + RETRY_FIRST),
            "a success resets the backoff"
        );
    }

    fn inputs() -> StatusInputs {
        StatusInputs {
            available: true,
            writing: false,
            failed: false,
            changed: false,
        }
    }

    #[test]
    fn edits_that_leave_the_file_as_it_is_read_as_up_to_date() {
        assert_eq!(status_of(inputs()), SaveStatus::UpToDate);
        let changed = StatusInputs {
            changed: true,
            ..inputs()
        };
        assert_eq!(status_of(changed), SaveStatus::Pending);
        let writing = StatusInputs {
            writing: true,
            ..inputs()
        };
        assert_eq!(status_of(writing), SaveStatus::Pending);
    }

    #[test]
    fn a_failure_outranks_a_waiting_change() {
        let failed = StatusInputs {
            failed: true,
            changed: true,
            ..inputs()
        };
        assert_eq!(status_of(failed), SaveStatus::Failed);
        let unavailable = StatusInputs {
            available: false,
            failed: true,
            ..inputs()
        };
        assert_eq!(status_of(unavailable), SaveStatus::Unavailable);
    }

    #[test]
    fn a_comparison_is_due_once_an_interval_has_passed() {
        let start = Instant::now();
        let mut saver = Saver::new();
        assert!(saver.comparison_due(start));
        saver.compared = Some(start);
        assert!(!saver.comparison_due(start + COMPARE_INTERVAL / 2));
        assert!(saver.comparison_due(start + COMPARE_INTERVAL));
    }
}
