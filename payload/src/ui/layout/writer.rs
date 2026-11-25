//! One retained layout writer, with a newest-only queue and successful-generation acknowledgement.
//! Durable replacement never holds the queue or UI locks. A shutdown timeout retains the handle.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};

use crate::config;

#[derive(Clone)]
pub(super) struct Snapshot {
    pub(super) generation: u64,
    pub(super) text: Arc<str>,
    path: PathBuf,
}

#[derive(Clone, Default)]
pub(super) struct Acknowledgement {
    pub(super) saved: Option<Snapshot>,
    pub(super) error: Option<String>,
}

pub(super) fn submit(path: PathBuf, text: String) -> Result<(), String> {
    OWNER.lock().submit(path, text.into(), replace)
}

pub(super) fn acknowledgement() -> Acknowledgement {
    OWNER.lock().acknowledgement()
}

#[must_use = "an unfinished layout writer requires the payload to stay mapped"]
pub(crate) fn shutdown() -> bool {
    let stopped = shutdown_owner(&OWNER, Duration::from_secs(10));
    if !stopped {
        tracing::error!("UI layout: writer has not exited; keeping the payload mapped");
    }
    stopped
}

static OWNER: Mutex<Writer> = Mutex::new(Writer::new());

struct Writer {
    shared: Option<Arc<Shared>>,
    handle: Option<JoinHandle<()>>,
    closed: bool,
}

impl Writer {
    const fn new() -> Self {
        Self {
            shared: None,
            handle: None,
            closed: false,
        }
    }

    fn submit(
        &mut self,
        path: PathBuf,
        text: Arc<str>,
        write: impl FnMut(&Snapshot) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        self.reap();
        if self.closed {
            return Err("UI layout: shutdown has closed writer admission".into());
        }
        let shared = self
            .shared
            .get_or_insert_with(|| Arc::new(Shared::default()));
        {
            let mut queue = shared.queue.lock();
            let same = queue
                .latest
                .as_ref()
                .is_some_and(|latest| latest.path == path && latest.text == text);
            if same && (queue.pending.is_some() || queue.in_flight || queue.ack.error.is_none()) {
                return Ok(());
            }
            let generation = queue
                .next
                .checked_add(1)
                .ok_or("UI layout: writer generation exhausted")?;
            queue.next = generation;
            let snapshot = Snapshot {
                generation,
                text,
                path,
            };
            queue.latest = Some(snapshot.clone());
            queue.pending = Some(snapshot);
        }
        if self.handle.is_none() {
            let thread_shared = Arc::clone(shared);
            match std::thread::Builder::new()
                .name("jc3vr-ui-layout".into())
                .spawn(move || run(thread_shared, write))
            {
                Ok(handle) => self.handle = Some(handle),
                Err(error) => {
                    let message = format!("UI layout: could not spawn writer: {error}");
                    let mut queue = shared.queue.lock();
                    queue.pending = None;
                    queue.ack.error = Some(message.clone());
                    return Err(message);
                }
            }
        }
        shared.ready.notify_one();
        Ok(())
    }

    fn acknowledgement(&self) -> Acknowledgement {
        self.shared
            .as_ref()
            .map(|shared| shared.queue.lock().ack.clone())
            .unwrap_or_default()
    }

    fn close(&mut self) {
        self.closed = true;
        if let Some(shared) = &self.shared {
            shared.queue.lock().closed = true;
            shared.ready.notify_one();
        }
    }

    fn reap(&mut self) {
        if self.handle.as_ref().is_some_and(JoinHandle::is_finished)
            && self.handle.take().unwrap().join().is_err()
        {
            self.shared.as_ref().unwrap().queue.lock().ack.error =
                Some("UI layout: writer thread panicked".into());
        }
    }
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}

#[derive(Default)]
struct Queue {
    next: u64,
    latest: Option<Snapshot>,
    pending: Option<Snapshot>,
    in_flight: bool,
    closed: bool,
    ack: Acknowledgement,
}

fn replace(snapshot: &Snapshot) -> Result<(), String> {
    config::write::replace(&snapshot.path, snapshot.text.as_bytes()).map_err(|error| {
        format!(
            "UI layout: could not replace {}: {error}",
            snapshot.path.display()
        )
    })
}

fn run(shared: Arc<Shared>, mut write: impl FnMut(&Snapshot) -> Result<(), String>) {
    loop {
        let snapshot = {
            let mut queue = shared.queue.lock();
            loop {
                if let Some(snapshot) = queue.pending.take() {
                    queue.in_flight = true;
                    break snapshot;
                }
                if queue.closed {
                    return;
                }
                shared.ready.wait(&mut queue);
            }
        };
        let result = catch_unwind(AssertUnwindSafe(|| write(&snapshot)))
            .unwrap_or_else(|_| Err("UI layout: writer panicked during replacement".into()));
        if let Err(error) = &result {
            tracing::warn!(%error, "UI layout: layout was not saved");
        }
        let mut queue = shared.queue.lock();
        queue.in_flight = false;
        match result {
            Ok(()) => {
                queue.ack.saved = Some(snapshot);
                queue.ack.error = None;
            }
            Err(error) => queue.ack.error = Some(error),
        }
    }
}

fn shutdown_owner(owner: &Mutex<Writer>, budget: Duration) -> bool {
    owner.lock().close();
    let deadline = Instant::now() + budget;
    loop {
        {
            let mut owner = owner.lock();
            owner.reap();
            if owner.handle.is_none() {
                return owner.shared.as_ref().is_none_or(|shared| {
                    let queue = shared.queue.lock();
                    queue.pending.is_none() && !queue.in_flight
                });
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(
            Duration::from_millis(1).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

#[cfg(test)]
mod tests;
