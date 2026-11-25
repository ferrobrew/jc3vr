//! One retained sidecar writer serializes atomic replacements outside game and UI locks.
//!
//! The queue holds one in-flight snapshot and only the newest pending sequence. Shutdown closes
//! admission and drains both before a confirmed join permits unload. A timeout retains the owner;
//! the caller must pin the module. The worker reports each attempt only after releasing its queue lock.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};

use crate::config::write;

pub(super) struct Snapshot {
    pub(super) path: PathBuf,
    pub(super) text: String,
    pub(super) sequence: u64,
    pub(super) count: usize,
}

#[derive(Clone, Default)]
pub(super) struct WriteStatus {
    pub(super) sequence: u64,
    pub(super) path: Option<PathBuf>,
    pub(super) writes: u64,
    pub(super) last_written: usize,
    pub(super) error: Option<String>,
}

pub(super) fn submit(snapshot: Snapshot) {
    let mut owner = OWNER.lock();
    if owner.closed {
        return;
    }
    if let Err(error) = owner.submit(snapshot) {
        tracing::warn!(target: "controllers", %error, "inventory: sidecar snapshot refused");
    }
}

pub(super) fn status() -> WriteStatus {
    OWNER.lock().status()
}

#[must_use = "an unfinished sidecar writer requires the payload to stay mapped"]
pub(crate) fn shutdown() -> bool {
    let stopped = shutdown_owner(&OWNER, SHUTDOWN_BUDGET);
    if !stopped {
        tracing::error!(target: "controllers", "inventory: sidecar writer has not exited; keeping the payload mapped");
    }
    stopped
}

const SHUTDOWN_BUDGET: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
static OWNER: Mutex<SidecarWriter> = Mutex::new(SidecarWriter::new());

fn shutdown_owner(owner: &Mutex<SidecarWriter>, budget: Duration) -> bool {
    owner.lock().close();
    let deadline = Instant::now() + budget;
    loop {
        {
            let mut owner = owner.lock();
            owner.reap();
            if owner.handle.is_none() {
                return owner
                    .shared
                    .as_ref()
                    .is_none_or(|shared| shared.queue.lock().pending.is_none());
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

struct SidecarWriter {
    shared: Option<Arc<Shared>>,
    handle: Option<JoinHandle<()>>,
    closed: bool,
}

impl SidecarWriter {
    const fn new() -> Self {
        Self {
            shared: None,
            handle: None,
            closed: false,
        }
    }

    fn submit(&mut self, snapshot: Snapshot) -> Result<(), String> {
        self.submit_with(snapshot, replace, || {})
    }

    /// Queue `snapshot` for `write`, starting the worker if it is not running. The worker calls
    /// `attempted` after each attempt, once its queue lock is released.
    fn submit_with(
        &mut self,
        snapshot: Snapshot,
        write: impl FnMut(&Snapshot) -> Result<(), String> + Send + 'static,
        attempted: impl FnMut() + Send + 'static,
    ) -> Result<(), String> {
        self.reap();
        if self.closed {
            return Err("inventory: sidecar: shutdown has closed admission".into());
        }
        if self.shared.is_none() {
            self.shared = Some(Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                ready: Condvar::new(),
            }));
        }
        let shared = self.shared.as_ref().unwrap();
        {
            let mut queue = shared.queue.lock();
            if snapshot.sequence <= queue.latest {
                return Ok(());
            }
            queue.latest = snapshot.sequence;
            queue.pending = Some(snapshot);
        }
        if self.handle.is_none() {
            let thread_shared = Arc::clone(shared);
            match std::thread::Builder::new()
                .name("jc3vr-inventory-sidecar".into())
                .spawn(move || run(thread_shared, write, attempted))
            {
                Ok(handle) => self.handle = Some(handle),
                Err(error) => {
                    let message = format!("inventory: sidecar: could not spawn writer: {error}");
                    let mut queue = shared.queue.lock();
                    queue.status.path =
                        queue.pending.as_ref().map(|snapshot| snapshot.path.clone());
                    queue.status.sequence = queue.latest;
                    queue.status.error = Some(message.clone());
                    queue.pending = None;
                    return Err(message);
                }
            }
        }
        shared.ready.notify_one();
        Ok(())
    }

    fn close(&mut self) {
        self.closed = true;
        if let Some(shared) = &self.shared {
            shared.queue.lock().closed = true;
            shared.ready.notify_one();
        }
    }

    fn reap(&mut self) -> bool {
        if !self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            return false;
        }
        let result = self.handle.take().unwrap().join();
        if result.is_err() {
            let mut queue = self.shared.as_ref().unwrap().queue.lock();
            queue.status.error = Some("inventory: sidecar: writer thread panicked".into());
        }
        true
    }

    fn status(&self) -> WriteStatus {
        self.shared
            .as_ref()
            .map(|shared| shared.queue.lock().status.clone())
            .unwrap_or_default()
    }
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}

#[derive(Default)]
struct Queue {
    pending: Option<Snapshot>,
    latest: u64,
    closed: bool,
    status: WriteStatus,
}

fn replace(snapshot: &Snapshot) -> Result<(), String> {
    write::replace(&snapshot.path, snapshot.text.as_bytes())
        .map_err(|error| format!("inventory: sidecar: {error}"))
}

fn run(
    shared: Arc<Shared>,
    mut write: impl FnMut(&Snapshot) -> Result<(), String>,
    mut attempted: impl FnMut(),
) {
    loop {
        let snapshot = {
            let mut queue = shared.queue.lock();
            loop {
                if let Some(snapshot) = queue.pending.take() {
                    break snapshot;
                }
                if queue.closed {
                    return;
                }
                shared.ready.wait(&mut queue);
            }
        };
        let result = catch_unwind(AssertUnwindSafe(|| write(&snapshot))).unwrap_or_else(|_| {
            Err("inventory: sidecar: writer panicked while replacing the file".into())
        });
        {
            let mut queue = shared.queue.lock();
            queue.status.sequence = snapshot.sequence;
            queue.status.path = Some(snapshot.path.clone());
            match &result {
                Ok(()) => {
                    queue.status.writes += 1;
                    queue.status.last_written = snapshot.count;
                    queue.status.error = None;
                }
                Err(error) => queue.status.error = Some(error.clone()),
            }
        }
        match result {
            Ok(()) => {
                tracing::info!(target: "controllers", carried = snapshot.count, path = %snapshot.path.display(), "inventory: wrote the save sidecar")
            }
            Err(error) => {
                tracing::warn!(target: "controllers", %error, "inventory: the save sidecar was not written")
            }
        }
        attempted();
    }
}

#[cfg(test)]
mod tests;
