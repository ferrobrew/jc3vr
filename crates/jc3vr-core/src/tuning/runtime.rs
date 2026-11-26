//! The tuning's life in the payload: the startup load, the watcher that reloads edited files, the
//! game thread's publication of what it reloaded, and the watcher's stop at shutdown.

use std::{
    fs,
    path::PathBuf,
    sync::{
        OnceLock,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

use parking_lot::Mutex;

use crate::tuning::{
    error::{FileCause, StartupError, TuningError},
    file::AnyTuningFile,
    source::{Source, packaged_root},
};

/// Loads every file in `files` from `source`, publishes them, and starts the watcher.
///
/// Every file is read before any is published, and the error lists every file that failed, so a
/// startup that stops says everything that stopped it. Called once, before anything reads the
/// tuning.
pub fn init(
    files: &'static [&'static dyn AnyTuningFile],
    source: Source,
) -> Result<(), StartupError> {
    let fail = |error| StartupError {
        errors: vec![error],
    };
    let root = match source {
        Source::Packaged => packaged_root().map_err(fail)?,
        Source::SourceTree => PathBuf::new(),
    };

    let mut watched = Vec::with_capacity(files.len());
    let mut errors = Vec::new();
    for &file in files {
        match source.path(file.location(), &root) {
            Ok(path) => match load(file, &path) {
                Ok(seen) => watched.push(Watched { file, path, seen }),
                Err(error) => errors.push(error),
            },
            Err(error) => errors.push(error),
        }
    }
    if !errors.is_empty() {
        return Err(StartupError { errors });
    }
    for watched in &watched {
        watched.file.commit();
        tracing::info!(path = %watched.path.display(), "tuning: loaded {}", watched.file.location());
    }
    FILES.set(files).ok();

    let (stop, stopped) = mpsc::channel();
    let handle = thread::Builder::new()
        .name("jc3vr-tuning".into())
        .spawn(move || watch(watched, stopped))
        .map_err(|error| fail(TuningError::Watcher(error)))?;
    *WATCHER.lock() = Some(Watcher { stop, handle });
    Ok(())
}

/// Publishes the values the watcher reloaded since the last call. Runs on the game thread once per
/// frame, before the frame's other work, so a frame reads one version of each file throughout.
pub fn tick() {
    let Some(files) = FILES.get() else {
        return;
    };
    for file in *files {
        if file.commit() {
            tracing::info!("tuning: applied the edit to {}", file.location());
        }
    }
}

/// Stops the watcher and waits for it, up to a bounded budget. Returns whether the thread is
/// confirmed stopped (or never ran); `false` means the payload must stay mapped.
#[must_use = "a watcher that did not stop means the payload must stay mapped"]
pub fn shutdown() -> bool {
    let Some(Watcher { stop, handle }) = WATCHER.lock().take() else {
        return true;
    };
    drop(stop);
    let deadline = Instant::now() + SHUTDOWN_BUDGET;
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            tracing::error!(
                "tuning: the watcher did not stop in time; keeping the payload mapped rather than \
                 unmapping under a live thread"
            );
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = handle.join();
    true
}

/// How often the watcher looks at the files. An edit applies within this of its save.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How long [`shutdown`] waits for the watcher: several polls, of which one read is the most the
/// watcher can be in the middle of.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(2);

static FILES: OnceLock<&'static [&'static dyn AnyTuningFile]> = OnceLock::new();
static WATCHER: Mutex<Option<Watcher>> = Mutex::new(None);

struct Watcher {
    /// Dropped to stop the watcher.
    stop: Sender<()>,
    handle: JoinHandle<()>,
}

/// A file the watcher looks at, and what it saw there last.
pub(crate) struct Watched {
    file: &'static dyn AnyTuningFile,
    path: PathBuf,
    seen: Seen,
}

/// What the watcher last saw of a file: its stamp, to notice a change without reading it, and its
/// text, so a save that changes nothing (or only the stamp) is not reloaded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Seen {
    stamp: Option<Stamp>,
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    modified: SystemTime,
    len: u64,
}

fn stamp(path: &PathBuf) -> Option<Stamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(Stamp {
        modified: metadata.modified().ok()?,
        len: metadata.len(),
    })
}

/// Reads and stages `file` from `path` at startup.
fn load(file: &dyn AnyTuningFile, path: &PathBuf) -> Result<Seen, TuningError> {
    let stamp = stamp(path);
    let text = fs::read_to_string(path).map_err(|error| FileCause::read(error).at(path))?;
    file.stage(&text).map_err(|cause| cause.at(path))?;
    Ok(Seen { stamp, text })
}

fn watch(mut watched: Vec<Watched>, stopped: mpsc::Receiver<()>) {
    loop {
        match stopped.recv_timeout(POLL_INTERVAL) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
        for watched in &mut watched {
            poll(watched);
        }
    }
}

/// Looks at one file and stages its text when it changed. A file that cannot be read or does not
/// hold valid tuning keeps the values already published; the next change is tried again.
fn poll(watched: &mut Watched) {
    let stamp = stamp(&watched.path);
    if stamp == watched.seen.stamp {
        return;
    }
    watched.seen.stamp = stamp;
    let text = match fs::read_to_string(&watched.path) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(
                "{}; keeping the previous values",
                FileCause::read(error).at(&watched.path)
            );
            return;
        }
    };
    if text == watched.seen.text {
        return;
    }
    match watched.file.stage(&text) {
        Ok(()) => {
            tracing::info!(
                "tuning: reloaded {}; it applies on the next frame",
                watched.file.location()
            );
            watched.seen.text = text;
        }
        Err(cause) => tracing::warn!("{}; keeping the previous values", cause.at(&watched.path)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A file as the startup leaves it for the watcher: loaded and staged.
    pub(crate) fn watched(file: &'static dyn AnyTuningFile, path: PathBuf) -> Watched {
        let seen = load(file, &path).unwrap();
        Watched { file, path, seen }
    }

    /// Polls as though the file's stamp changed, which a rewrite within the file system's stamp
    /// resolution, to the same length, does not show.
    pub(crate) fn force_poll(watched: &mut Watched) {
        watched.seen.stamp = None;
        poll(watched);
    }
}
