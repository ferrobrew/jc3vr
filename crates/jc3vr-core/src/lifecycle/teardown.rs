//! The eject's worker teardown and its settle waits.
//!
//! Every thread the payload spawns must have exited before the image unmaps: a thread still
//! executing in an unmapped image has no return address, so it parks forever holding whatever it
//! took, and the process wedges with nothing runnable and nothing logged. Each such worker
//! registers its stop here, and the shutdown thread runs the stops once, in registration order,
//! after the cleanups. A stop that cannot confirm its thread gone does not end the teardown: the
//! later workers still stop, and the caller keeps the image mapped ([`crate::module::pin`]).
//!
//! The settle wait ([`settle_frames`]) lets the engine run a few frames between teardown steps,
//! counted by the engine's own per-frame counter, which it advances whether or not any detour is
//! installed.

use std::{
    fmt, thread,
    time::{Duration, Instant},
};

use jc3gi::graphics_engine::graphics_engine::get_render_frame_counters;
use parking_lot::Mutex;

use crate::core_tuning::CORE;

/// A worker's stop: close its admission, wait for its thread within the worker's own budget, and
/// return whether the thread is confirmed gone (or never ran). `false` means the image must stay
/// mapped.
pub type StopWorker = fn() -> bool;

/// Register a worker's stop, under a name the log and the order test use. Stops run in
/// registration order, so whoever registers decides the order.
pub fn register_worker(
    name: &'static str,
    stop: StopWorker,
) -> Result<(), WorkerRegistrationError> {
    WORKERS.lock().register(name, stop)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WorkerRegistrationError {
    Duplicate(&'static str),
    TeardownStarted(&'static str),
}

impl fmt::Display for WorkerRegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate(name) => write!(f, "worker teardown: {name} is already registered"),
            Self::TeardownStarted(name) => {
                write!(
                    f,
                    "worker teardown: {name} registered after the teardown started"
                )
            }
        }
    }
}

impl std::error::Error for WorkerRegistrationError {}

/// Stop every registered worker once, in registration order, and return the names of those that
/// did not confirm their thread gone. Runs only after the shutdown commit.
#[must_use = "a worker that did not stop means the payload must stay mapped"]
pub fn stop_workers() -> Vec<&'static str> {
    assert!(
        crate::is_shutting_down(),
        "worker teardown requires committed shutdown"
    );
    let stalled = WORKERS.lock().close();
    stop_all(&stalled)
}

/// Wait for the engine to run the teardown tuning's number of frames, or for its budget to pass,
/// whichever is first. `reason` names what the frames are for in the log line a budget that runs
/// out leaves (the frame loop does not run while the window is in the background outside VR).
pub fn settle_frames(reason: &str) {
    let tuning = CORE.get().teardown;
    let budget = Duration::from_millis(tuning.settle_budget_ms);
    // SAFETY: reads the engine's per-frame counter, a static word the game thread advances in the
    // `Draw` prologue; the read is volatile because another thread writes it.
    let counter =
        || unsafe { std::ptr::read_volatile(&raw const get_render_frame_counters().m_Counter) };
    if !wait_for_frames(counter, tuning.settle_frames, budget) {
        tracing::info!(
            "worker teardown: the frame loop did not advance within {} ms for {reason}; \
             continuing",
            tuning.settle_budget_ms
        );
    }
}

static WORKERS: Mutex<Workers> = Mutex::new(Workers::new());

/// How long the settle wait sleeps between reads of the frame counter: well under a frame, so the
/// wait ends within a millisecond of its last frame.
const SETTLE_POLL: Duration = Duration::from_millis(1);

struct Worker {
    name: &'static str,
    stop: StopWorker,
}

struct Workers {
    entries: Vec<Worker>,
    closed: bool,
}

impl Workers {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
            closed: false,
        }
    }

    fn register(
        &mut self,
        name: &'static str,
        stop: StopWorker,
    ) -> Result<(), WorkerRegistrationError> {
        if self.closed {
            return Err(WorkerRegistrationError::TeardownStarted(name));
        }
        if self.entries.iter().any(|worker| worker.name == name) {
            return Err(WorkerRegistrationError::Duplicate(name));
        }
        self.entries.push(Worker { name, stop });
        Ok(())
    }

    /// Close registration and hand out the stops, so none runs under the registry lock.
    fn close(&mut self) -> Vec<Worker> {
        self.closed = true;
        std::mem::take(&mut self.entries)
    }
}

fn stop_all(workers: &[Worker]) -> Vec<&'static str> {
    let mut stalled = Vec::new();
    for worker in workers {
        if !(worker.stop)() {
            tracing::error!(
                "worker teardown: {} did not stop; the payload stays mapped",
                worker.name
            );
            stalled.push(worker.name);
        }
    }
    stalled
}

fn wait_for_frames(counter: impl Fn() -> u32, frames: u32, budget: Duration) -> bool {
    let start = counter();
    let deadline = Instant::now() + budget;
    loop {
        if counter().wrapping_sub(start) >= frames {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(SETTLE_POLL);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use pretty_assertions::assert_eq;

    use super::*;

    static CALLS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    fn first() -> bool {
        CALLS.lock().push("first");
        true
    }

    fn stalls() -> bool {
        CALLS.lock().push("stalls");
        false
    }

    fn last() -> bool {
        CALLS.lock().push("last");
        true
    }

    #[test]
    fn stops_run_in_registration_order_past_a_stall_and_only_once() {
        let mut workers = Workers::new();
        workers.register("first", first).unwrap();
        workers.register("stalls", stalls).unwrap();
        workers.register("last", last).unwrap();

        let stalled = stop_all(&workers.close());

        assert_eq!(stalled, vec!["stalls"]);
        assert_eq!(*CALLS.lock(), vec!["first", "stalls", "last"]);
        assert!(workers.close().is_empty());
    }

    #[test]
    fn registration_refuses_duplicates_and_late_workers() {
        let mut workers = Workers::new();
        workers.register("first", first).unwrap();
        assert_eq!(
            workers.register("first", last),
            Err(WorkerRegistrationError::Duplicate("first"))
        );
        let _ = workers.close();
        assert_eq!(
            workers.register("last", last),
            Err(WorkerRegistrationError::TeardownStarted("last"))
        );
    }

    #[test]
    fn the_settle_wait_ends_when_the_frames_pass_and_tolerates_wrap() {
        let counter = AtomicU32::new(u32::MAX - 1);
        let advanced = wait_for_frames(
            || counter.fetch_add(1, Ordering::Relaxed),
            3,
            Duration::from_secs(5),
        );
        assert!(advanced);
    }

    #[test]
    fn the_settle_wait_gives_up_at_its_budget_on_a_stalled_frame_loop() {
        assert!(!wait_for_frames(|| 7, 1, Duration::from_millis(5)));
    }
}
