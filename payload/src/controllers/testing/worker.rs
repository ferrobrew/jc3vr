//! One-at-a-time persistence work and its unload-safe thread lifetime.

use std::{
    path::PathBuf,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use crate::{
    config::CONFIG,
    controllers::testing::{self, ControllerFile, Error},
};

/// An explicit persistence request, with an immutable snapshot for saves.
pub(crate) enum Request {
    Save(Box<ControllerFile>),
    Load,
}

/// The persistence operation's current UI-visible state.
#[derive(Clone)]
pub(crate) enum Status {
    Idle,
    Queued,
    Running,
    Finished(Result<PathBuf, Error>),
    ShuttingDown,
}

/// Queue one operation unless another operation or shutdown owns the worker.
pub(crate) fn queue(request: Request) -> bool {
    let mut state = STATE.lock();
    state.reap();
    if crate::is_shutting_down()
        || state.shutting_down
        || state.handle.is_some()
        || state.pending.is_some()
    {
        return false;
    }
    state.pending = Some(request);
    state.status = Status::Queued;
    true
}

/// Start queued work outside the configuration lock.
pub(crate) fn start_pending() {
    let mut state = STATE.lock();
    state.reap();
    if crate::is_shutting_down() || state.shutting_down || state.handle.is_some() {
        return;
    }
    let Some(request) = state.pending.take() else {
        return;
    };
    // Keep the lifecycle lock through registration so shutdown cannot miss a new thread.
    match thread::Builder::new()
        .name("controller-settings".into())
        .spawn(move || run(request))
    {
        Ok(handle) => {
            state.handle = Some(handle);
            state.status = Status::Running;
        }
        Err(error) => {
            state.status = Status::Finished(Err(Error::Io {
                operation: "start the settings worker",
                detail: error.to_string(),
            }));
        }
    }
}

/// Read status, joining a completed operation before exposing its result.
pub(crate) fn status() -> Status {
    let mut state = STATE.lock();
    state.reap();
    if crate::is_shutting_down() || state.shutting_down {
        Status::ShuttingDown
    } else {
        state.status.clone()
    }
}

/// Stop accepting work, discard queued requests, and wait up to ten seconds for the worker.
/// A false return requires the caller to keep the payload image mapped.
#[must_use = "a worker that did not finish means the payload must stay mapped"]
pub fn shutdown() -> bool {
    shutdown_with_budget(Duration::from_secs(10))
}

static STATE: Mutex<State> = Mutex::new(State::new());

struct State {
    shutting_down: bool,
    pending: Option<Request>,
    handle: Option<JoinHandle<Result<PathBuf, Error>>>,
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

    fn stop(&mut self) {
        self.shutting_down = true;
        self.pending = None;
    }

    fn reap(&mut self) {
        if self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            let result = self.handle.take().unwrap().join().unwrap_or_else(|_| {
                Err(Error::Io {
                    operation: "run the settings worker",
                    detail: "the worker panicked".into(),
                })
            });
            match &result {
                Ok(path) => {
                    tracing::info!(path = %path.display(), "Controller settings operation completed")
                }
                Err(error) => tracing::warn!(%error, "Controller settings operation failed"),
            }
            self.status = Status::Finished(result);
        }
    }
}

fn shutdown_with_budget(budget: Duration) -> bool {
    STATE.lock().stop();
    let started = Instant::now();
    loop {
        {
            let mut state = STATE.lock();
            state.reap();
            if state.handle.is_none() {
                return true;
            }
        }
        if started.elapsed() >= budget {
            tracing::error!(
                "controller settings: worker did not finish before shutdown deadline; keep the payload mapped"
            );
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn run(request: Request) -> Result<PathBuf, Error> {
    if crate::is_shutting_down() || STATE.lock().shutting_down {
        return Err(cancelled());
    }
    let path = testing::file_path().ok_or(Error::PathUnavailable)?;
    match request {
        Request::Save(file) => testing::save_snapshot_to_path(&path, &file),
        Request::Load => {
            let text = std::fs::read_to_string(&path).map_err(|error| Error::Io {
                operation: "read the settings file",
                detail: format!("{}: {error}", path.display()),
            })?;
            let file = testing::parse(&text)?;
            // Match the UI's CONFIG-before-STATE lock order. Shutdown cannot cross this apply.
            let mut config = CONFIG.lock();
            let state = STATE.lock();
            if crate::is_shutting_down() || state.shutting_down {
                return Err(cancelled());
            }
            testing::apply(&file, &mut config)?;
            Ok(path)
        }
    }
}

fn cancelled() -> Error {
    Error::Io {
        operation: "run the settings operation",
        detail: "shutdown cancels the operation".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn shutdown_rejects_queueing_and_starting() {
        assert!(queue(Request::Load));
        assert!(shutdown_with_budget(Duration::ZERO));
        assert!(!queue(Request::Load));
        start_pending();
        assert!(matches!(status(), Status::ShuttingDown));
        let state = STATE.lock();
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
            Ok(PathBuf::from("settings.json"))
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
