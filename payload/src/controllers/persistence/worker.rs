//! One-at-a-time persistence work and its unload-safe thread lifetime.

use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use crate::{
    config::CONFIG,
    controllers::persistence::{self, ControllerFile, Error, ShapeReport},
};

/// An explicit persistence request, with an immutable snapshot for saves.
pub(crate) enum Request {
    Save(Box<ControllerFile>),
    Load,
}

/// What a finished operation did: the file it wrote or read, and for a load, the settings the file
/// lacked or had in excess.
#[derive(Clone)]
pub(crate) struct Outcome {
    pub path: PathBuf,
    pub report: ShapeReport,
}

/// The persistence operation's current UI-visible state.
#[derive(Clone)]
pub(crate) enum Status {
    Idle,
    Queued,
    Running,
    Finished(Result<Outcome, Arc<Error>>),
    ShuttingDown,
}

/// Queue one operation unless another operation or shutdown owns the worker.
pub(crate) fn queue(request: Request) -> bool {
    STATE.lock().queue(request)
}

/// Start queued work outside the configuration lock.
pub(crate) fn start_pending() {
    STATE.lock().start_pending();
}

/// Read status, joining a completed operation before exposing its result.
pub(crate) fn status() -> Status {
    STATE.lock().status()
}

/// Stop accepting work, discard queued requests, and wait up to ten seconds for the worker.
/// A false return requires the caller to keep the payload image mapped.
#[must_use = "a worker that did not finish means the payload must stay mapped"]
pub fn shutdown() -> bool {
    shutdown_with_budget(&STATE, Duration::from_secs(10))
}

static STATE: Mutex<State> = Mutex::new(State::new());

struct State {
    shutting_down: bool,
    pending: Option<Request>,
    handle: Option<JoinHandle<Result<Outcome, Error>>>,
    status: Status,
}

impl State {
    const fn new() -> Self {
        Self {
            shutting_down: false,
            pending: None,
            handle: None,
            status: Status::Idle,
        }
    }

    fn closed(&self) -> bool {
        crate::is_shutting_down() || self.shutting_down
    }

    fn queue(&mut self, request: Request) -> bool {
        self.reap();
        if self.closed() || self.handle.is_some() || self.pending.is_some() {
            return false;
        }
        self.pending = Some(request);
        self.status = Status::Queued;
        true
    }

    fn start_pending(&mut self) {
        self.reap();
        if self.closed() || self.handle.is_some() {
            return;
        }
        let Some(request) = self.pending.take() else {
            return;
        };
        // The caller holds the lifecycle lock through registration, so shutdown cannot miss a new
        // thread.
        match thread::Builder::new()
            .name("controller-settings".into())
            .spawn(move || run(request))
        {
            Ok(handle) => {
                self.handle = Some(handle);
                self.status = Status::Running;
            }
            Err(source) => self.status = Status::Finished(Err(Arc::new(Error::Spawn(source)))),
        }
    }

    fn status(&mut self) -> Status {
        self.reap();
        if self.closed() {
            Status::ShuttingDown
        } else {
            self.status.clone()
        }
    }

    fn stop(&mut self) {
        self.shutting_down = true;
        self.pending = None;
    }

    fn reap(&mut self) {
        let Some(handle) = self.handle.take_if(|handle| handle.is_finished()) else {
            return;
        };
        let result = handle.join().unwrap_or(Err(Error::WorkerPanicked));
        match &result {
            Ok(outcome) if outcome.report.is_empty() => {
                tracing::info!(
                    path = %outcome.path.display(),
                    "controller settings: operation completed",
                );
            }
            Ok(outcome) => tracing::warn!(
                path = %outcome.path.display(),
                defaulted = ?outcome.report.defaulted,
                ignored = ?outcome.report.ignored,
                "controller settings: loaded, defaulting missing and ignoring unknown settings",
            ),
            Err(error) => tracing::warn!("{error}"),
        }
        self.status = Status::Finished(result.map_err(Arc::new));
    }
}

fn shutdown_with_budget(state: &Mutex<State>, budget: Duration) -> bool {
    state.lock().stop();
    let started = Instant::now();
    loop {
        {
            let mut state = state.lock();
            state.reap();
            if state.handle.is_none() {
                return true;
            }
        }
        if started.elapsed() >= budget {
            tracing::error!(
                "controller settings: the worker outlived the shutdown deadline; keeping the payload \
                 mapped"
            );
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn run(request: Request) -> Result<Outcome, Error> {
    if STATE.lock().closed() {
        return Err(Error::Cancelled);
    }
    match request {
        Request::Save(file) => {
            let path = persistence::file_path().ok_or(Error::PathUnavailable)?;
            persistence::save_snapshot_to_path(&path, &file)?;
            Ok(Outcome {
                path,
                report: ShapeReport::default(),
            })
        }
        Request::Load => {
            let path = persistence::load_path().ok_or(Error::PathUnavailable)?;
            let text = fs::read_to_string(&path).map_err(|source| Error::Io {
                operation: "read",
                path: path.clone(),
                source,
            })?;
            let parsed = persistence::parse(&text)?;
            // Match the UI's CONFIG-before-STATE lock order. Shutdown cannot cross this apply.
            let mut config = CONFIG.lock();
            let state = STATE.lock();
            if state.closed() {
                return Err(Error::Cancelled);
            }
            persistence::apply(&parsed.file, &mut config)?;
            Ok(Outcome {
                path,
                report: parsed.report,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn shutdown_rejects_queueing_and_starting() {
        let state = Mutex::new(State::new());
        assert!(state.lock().queue(Request::Load));
        assert!(shutdown_with_budget(&state, Duration::ZERO));
        assert!(!state.lock().queue(Request::Load));
        state.lock().start_pending();
        assert!(matches!(state.lock().status(), Status::ShuttingDown));
        let state = state.lock();
        assert!(state.pending.is_none());
        assert!(state.handle.is_none());
    }

    #[test]
    fn stopping_discards_queued_work_and_keeps_the_live_handle() {
        let mut state = State::new();
        state.pending = Some(Request::Load);
        let (release, wait) = mpsc::channel();
        state.handle = Some(thread::spawn(move || {
            wait.recv().unwrap();
            Ok(Outcome {
                path: PathBuf::from("settings.json"),
                report: ShapeReport::default(),
            })
        }));
        state.stop();
        state.reap();
        assert!(state.shutting_down);
        assert!(state.pending.is_none());
        assert!(state.handle.is_some());
        release.send(()).unwrap();
        state.handle.take().unwrap().join().unwrap().unwrap();
    }
}
