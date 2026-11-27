//! The estimator's render-thread half: the GPU executor on the game's device, fed the window the
//! game thread posted, polled every frame without waiting, and its estimates published.
//!
//! The weights load on a thread of their own the first time a window arrives (reading and parsing
//! the file takes longer than a frame), and the render thread uploads them, as 16-bit floats, once
//! they are in. Missing or unreadable weights while the estimator is on are a panic naming the path
//! the file was expected at and how to produce it: the weights are part of the mod's installation,
//! so their absence is a broken install, not a state to run in. Turning the estimator off drops the
//! executor and its GPU resources.

use std::{
    collections::VecDeque,
    fmt, io,
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread::JoinHandle,
    time::Duration,
};

use body_estimator::{
    EgoPoser, WeightsError,
    egoposer::gpu::{GpuEgoPoser, GpuError, Readback, WeightPrecision},
};
use jc3vr_core::published::Stamp;
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};

use crate::controllers::{
    body::{
        config::BODY_REMAP_CONFIG,
        estimator::{
            Reading,
            config::{DEFAULT_WEIGHTS_FILE, WEIGHTS_SCRIPT},
            producer, publish, withdraw,
        },
    },
    roomscale::posture::EffectivePosture,
};

/// Why the estimator could not run.
#[derive(Debug)]
pub(crate) enum EstimatorError {
    /// Neither the settings nor the payload's own location name the weights' file.
    NoWeightsPath,
    /// The weights' file is not there.
    MissingWeights { path: PathBuf },
    /// The weights' file could not be loaded.
    Weights(WeightsError),
    /// The thread that loads the weights could not start.
    LoaderThread(io::Error),
    /// The loader thread ended without a result, which only a panic does; the join logs it.
    LoaderPanicked,
    /// The executor could not be created on the game's device, or a result could not be read.
    Gpu(GpuError),
}

impl fmt::Display for EstimatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "body estimator: ")?;
        match self {
            EstimatorError::NoWeightsPath => write!(
                f,
                "no weights path: set `controllers.body.estimator.weights_path`, or put \
                 {DEFAULT_WEIGHTS_FILE} next to the payload DLL ({WEIGHTS_SCRIPT} produces it)"
            ),
            EstimatorError::MissingWeights { path } => write!(
                f,
                "the weights are not at {}: produce them from the authors' checkpoint with \
                 {WEIGHTS_SCRIPT} (docs/mod/body/body-estimator.md), or set \
                 `controllers.body.estimator.weights_path`, or turn the estimator off",
                path.display()
            ),
            EstimatorError::Weights(error) => write!(f, "{error}"),
            EstimatorError::LoaderThread(error) => {
                write!(f, "could not start the weights' loader thread: {error}")
            }
            EstimatorError::LoaderPanicked => {
                write!(
                    f,
                    "the weights' loader thread panicked before it delivered the weights"
                )
            }
            EstimatorError::Gpu(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EstimatorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EstimatorError::Weights(error) => Some(error),
            EstimatorError::LoaderThread(error) => Some(error),
            EstimatorError::Gpu(error) => Some(error),
            EstimatorError::NoWeightsPath
            | EstimatorError::MissingWeights { .. }
            | EstimatorError::LoaderPanicked => None,
        }
    }
}

/// What the executor is doing, for the debug UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExecutorStatus {
    /// `off`, `waiting`, `loading`, `running`, or `failed`.
    pub stage: &'static str,
    /// The weights' file, once known.
    pub weights: Option<PathBuf>,
    /// The error that stopped it, while failed.
    pub error: Option<String>,
    pub submitted: u64,
    pub read: u64,
    /// Estimates the ring overwrote before they were read.
    pub dropped: u64,
    /// The CPU time the latest submission took to record.
    pub record: Option<Duration>,
}

/// Run one frame of the executor: take the posted window, record its pass, and publish what has
/// finished. Called once per frame on the render thread with the engine's context lock held.
pub fn render_tick(context: &ID3D11DeviceContext, device: &ID3D11Device) {
    let config = BODY_REMAP_CONFIG.lock().estimator.clone();
    let mut executor = EXECUTOR.lock();
    if !config.enabled {
        if !matches!(executor.stage, Stage::Off) {
            tracing::info!(target: "body", "body estimator: off");
            executor.stage = Stage::Off;
            executor.in_flight.clear();
        }
        return;
    }
    let path = config.weights(jc3vr_core::module::get_path());
    let now = Stamp::now();
    if executor.path != path {
        executor.path = path.clone();
        executor.stage = Stage::Off;
        executor.in_flight.clear();
        executor.logged_error = false;
    }
    let submission = producer::take_submission();
    executor.advance(device, submission.is_some());
    if let Stage::Ready(gpu) = &mut executor.stage {
        let collected = collect(gpu.as_mut(), context);
        let mut gpu_error = None;
        match collected {
            Ok(readbacks) => {
                for readback in readbacks {
                    executor.finish(readback, now);
                }
            }
            Err(error) => gpu_error = Some(error),
        }
        if let Some(error) = gpu_error {
            executor.fail(EstimatorError::Gpu(error));
            return;
        }
        if let (Stage::Ready(gpu), Some(submission)) = (&mut executor.stage, submission) {
            let started = Stamp::now();
            let inference = gpu.submit(context, &submission.window);
            let record = started.elapsed();
            executor.record = Some(record);
            executor.submitted += 1;
            executor.in_flight.push_back(InFlight {
                inference,
                window_end: submission.window_end,
                posture: submission.posture,
                submitted_at: now,
            });
            while executor.in_flight.len() > IN_FLIGHT_KEPT {
                executor.in_flight.pop_front();
            }
        }
    }
}

#[must_use = "a loader that did not finish means the payload must stay mapped"]
pub fn shutdown() -> bool {
    withdraw();
    stop(&EXECUTOR, SHUTDOWN_BUDGET)
}

/// The executor's state, for the debug UI.
pub fn status() -> ExecutorStatus {
    let enabled = BODY_REMAP_CONFIG.lock().estimator.enabled;
    let executor = EXECUTOR.lock();
    ExecutorStatus {
        stage: match executor.stage {
            Stage::Off if enabled => "waiting",
            Stage::Off => "off",
            Stage::Loading(_) => "loading",
            Stage::Ready(_) => "running",
            Stage::Failed { .. } => "failed",
        },
        weights: executor.path.clone(),
        error: match &executor.stage {
            Stage::Failed { error, .. } => Some(error.clone()),
            _ => None,
        },
        submitted: executor.submitted,
        read: executor.read,
        dropped: executor.dropped,
        record: executor.record,
    }
}

/// The weights' precision on the GPU: 16-bit halves the memory at no measurable cost.
const PRECISION: WeightPrecision = WeightPrecision::F16;

/// More submissions than the executor's ring holds are never in flight; a few spare covers the
/// overwritten ones until a later readback prunes them.
const IN_FLIGHT_KEPT: usize = 8;

const SHUTDOWN_BUDGET: Duration = Duration::from_secs(10);
const SHUTDOWN_POLL: Duration = Duration::from_millis(10);

enum Stage {
    /// Not started: off, or waiting for the first window.
    Off,
    /// The weights are loading on their own thread.
    Loading(Receiver<Result<EgoPoser, WeightsError>>),
    /// Running on the game's device.
    Ready(Box<GpuEgoPoser>),
    /// Stopped by an error.
    Failed { error: String },
}

/// A submitted inference not yet read back.
#[derive(Debug, Copy, Clone)]
struct InFlight {
    inference: u64,
    window_end: Stamp,
    posture: EffectivePosture,
    submitted_at: Stamp,
}

struct Executor {
    stage: Stage,
    path: Option<PathBuf>,
    in_flight: VecDeque<InFlight>,
    submitted: u64,
    read: u64,
    dropped: u64,
    record: Option<Duration>,
    /// Whether the current failure has been logged, so a retry that fails the same way is quiet.
    logged_error: bool,
    handles: Vec<JoinHandle<()>>,
    closed: bool,
    stopped: LoaderShutdown,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum LoaderShutdown {
    NotAsked,
    Joining,
    Done,
    Failed,
}

impl Executor {
    /// Start or load as the stage needs. `wanted` is whether a window arrived this frame:
    /// nothing loads before the first.
    fn advance(&mut self, device: &ID3D11Device, wanted: bool) {
        self.reap();
        match &self.stage {
            Stage::Off if wanted => self.start(),
            Stage::Loading(receiver) => match receiver.try_recv() {
                Ok(Ok(model)) => match GpuEgoPoser::new(device, &model, PRECISION) {
                    Ok(gpu) => {
                        tracing::info!(
                            target: "body",
                            weights = %self.path.as_deref().map_or_else(String::new, |p| p.display().to_string()),
                            precision = ?PRECISION,
                            estimates_shape = gpu.estimates_shape(),
                            "body estimator: running on the game's device",
                        );
                        self.logged_error = false;
                        self.stage = Stage::Ready(Box::new(gpu));
                    }
                    Err(error) => self.fail(EstimatorError::Gpu(error)),
                },
                Ok(Err(error)) => panic!("{}", EstimatorError::Weights(error)),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.fail(EstimatorError::LoaderPanicked),
            },
            _ => {}
        }
    }

    /// Look for the weights and start loading them. Weights that are not there are a broken
    /// installation, so they panic rather than leave the estimator quietly off.
    fn start(&mut self) {
        if self.closed {
            return;
        }
        let Some(path) = self.path.clone() else {
            panic!("{}", EstimatorError::NoWeightsPath);
        };
        if !path.is_file() {
            panic!("{}", EstimatorError::MissingWeights { path });
        }
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("jc3vr body estimator weights".into())
            .spawn(move || {
                // The receiver is gone only if the estimator stopped meanwhile.
                let _ = sender.send(EgoPoser::load(&path));
            });
        match spawned {
            Ok(handle) => {
                self.handles.push(handle);
                self.stage = Stage::Loading(receiver);
            }
            Err(error) => self.fail(EstimatorError::LoaderThread(error)),
        }
    }

    fn reap(&mut self) {
        let mut index = 0;
        while index < self.handles.len() {
            if self.handles[index].is_finished() {
                join_loader(self.handles.swap_remove(index));
            } else {
                index += 1;
            }
        }
    }

    /// Stop on `error`, logging it the first time.
    fn fail(&mut self, error: EstimatorError) {
        if !self.logged_error {
            tracing::error!(target: "body", "{error}");
            self.logged_error = true;
        }
        self.stage = Stage::Failed {
            error: error.to_string(),
        };
        self.in_flight.clear();
        withdraw();
    }

    /// Publish a finished inference, matched to its submission.
    fn finish(&mut self, readback: Readback, now: Stamp) {
        self.read += 1;
        while let Some(front) = self.in_flight.front().copied() {
            if front.inference > readback.inference {
                break;
            }
            self.in_flight.pop_front();
            if front.inference < readback.inference {
                self.dropped += 1;
                continue;
            }
            publish(Reading {
                output: readback.output,
                window_end: front.window_end,
                posture: front.posture,
                submitted_at: front.submitted_at,
                read_at: now,
            });
        }
    }
}

fn wait(handle: JoinHandle<()>, budget: Duration) -> bool {
    let started = Stamp::now();
    loop {
        if handle.is_finished() {
            return join_loader(handle);
        }
        if started.elapsed() >= budget {
            return false;
        }
        std::thread::sleep(SHUTDOWN_POLL);
    }
}

/// Join a finished loader thread, logging its panic if it had one. True when it ended cleanly.
fn join_loader(handle: JoinHandle<()>) -> bool {
    let Err(payload) = handle.join() else {
        return true;
    };
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a non-string payload");
    tracing::error!(target: "body", "body estimator: the weights' loader thread panicked: {message}");
    false
}

fn stop(executor: &Mutex<Executor>, budget: Duration) -> bool {
    {
        let mut executor = executor.lock();
        executor.closed = true;
        executor.stage = Stage::Off;
        executor.in_flight.clear();
    }
    let started = Stamp::now();
    loop {
        let handles = {
            let mut executor = executor.lock();
            match executor.stopped {
                LoaderShutdown::Done => return true,
                LoaderShutdown::Failed => return false,
                LoaderShutdown::Joining => None,
                LoaderShutdown::NotAsked => {
                    executor.stopped = LoaderShutdown::Joining;
                    Some(std::mem::take(&mut executor.handles))
                }
            }
        };
        if let Some(handles) = handles {
            let mut joined = true;
            for handle in handles {
                joined &= wait(handle, budget.saturating_sub(started.elapsed()));
            }
            let mut executor = executor.lock();
            if executor.stopped == LoaderShutdown::Joining {
                executor.stopped = if joined {
                    LoaderShutdown::Done
                } else {
                    LoaderShutdown::Failed
                };
            }
            let stopped = executor.stopped;
            drop(executor);
            if stopped == LoaderShutdown::Failed {
                tracing::error!(
                    "body estimator: the weights' loader is not confirmed stopped; leaving the \
                     payload mapped rather than unmapping under a live thread"
                );
            }
            return stopped == LoaderShutdown::Done;
        }
        if started.elapsed() >= budget {
            let mut executor = executor.lock();
            if executor.stopped == LoaderShutdown::Joining {
                executor.stopped = LoaderShutdown::Failed;
                tracing::error!(
                    "body estimator: another shutdown is still waiting on the weights' loader; \
                     leaving the payload mapped"
                );
            }
            return false;
        }
        std::thread::sleep(SHUTDOWN_POLL);
    }
}

/// Every estimate the GPU has finished, oldest first, without waiting.
fn collect(
    gpu: &mut GpuEgoPoser,
    context: &ID3D11DeviceContext,
) -> Result<Vec<Readback>, GpuError> {
    let mut readbacks = Vec::new();
    while let Some(readback) = gpu.poll(context)? {
        readbacks.push(readback);
    }
    Ok(readbacks)
}

static EXECUTOR: Mutex<Executor> = Mutex::new(Executor {
    stage: Stage::Off,
    path: None,
    in_flight: VecDeque::new(),
    submitted: 0,
    read: 0,
    dropped: 0,
    record: None,
    logged_error: false,
    handles: Vec::new(),
    closed: false,
    stopped: LoaderShutdown::NotAsked,
});

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use super::*;

    fn executor_with(handles: Vec<JoinHandle<()>>) -> Mutex<Executor> {
        Mutex::new(Executor {
            stage: Stage::Off,
            path: None,
            in_flight: VecDeque::new(),
            submitted: 0,
            read: 0,
            dropped: 0,
            record: None,
            logged_error: false,
            handles,
            closed: false,
            stopped: LoaderShutdown::NotAsked,
        })
    }

    fn flag() -> (Arc<AtomicBool>, Arc<AtomicBool>) {
        let flag = Arc::new(AtomicBool::new(false));
        (flag.clone(), flag)
    }

    fn finishing_after(delay: Duration, done: Arc<AtomicBool>) -> JoinHandle<()> {
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            done.store(true, Ordering::SeqCst);
        })
    }

    fn blocked_until(running: Arc<AtomicBool>) -> JoinHandle<()> {
        std::thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    }

    #[test]
    fn stopping_waits_for_a_loader_that_finishes_late() {
        let (done, finished) = flag();
        let executor = executor_with(vec![finishing_after(Duration::from_millis(50), done)]);
        assert!(stop(&executor, Duration::from_secs(5)));
        assert!(finished.load(Ordering::SeqCst));
        assert_eq!(executor.lock().stopped, LoaderShutdown::Done);
    }

    #[test]
    fn stopping_without_a_loader_is_true_and_repeats() {
        let executor = executor_with(Vec::new());
        assert!(stop(&executor, Duration::from_millis(50)));
        assert!(stop(&executor, Duration::from_millis(50)));
        let executor = executor.lock();
        assert!(executor.closed);
        assert_eq!(executor.stopped, LoaderShutdown::Done);
    }

    #[test]
    fn every_outstanding_loader_is_joined() {
        let (first, first_done) = flag();
        let (second, second_done) = flag();
        let executor = executor_with(vec![
            finishing_after(Duration::from_millis(30), first),
            finishing_after(Duration::from_millis(60), second),
        ]);
        assert!(stop(&executor, Duration::from_secs(5)));
        assert!(first_done.load(Ordering::SeqCst));
        assert!(second_done.load(Ordering::SeqCst));
        assert!(executor.lock().handles.is_empty());
    }

    #[test]
    fn a_restart_keeps_the_previous_loader_until_joined() {
        let (first, first_done) = flag();
        let (second, second_done) = flag();
        let mut executor = executor_with(Vec::new()).into_inner();
        executor
            .handles
            .push(finishing_after(Duration::from_millis(200), first));
        executor.reap();
        assert_eq!(executor.handles.len(), 1);
        executor
            .handles
            .push(finishing_after(Duration::from_millis(200), second));
        assert_eq!(executor.handles.len(), 2);
        assert!(!first_done.load(Ordering::SeqCst));
        let executor = Mutex::new(executor);
        assert!(stop(&executor, Duration::from_secs(5)));
        assert!(first_done.load(Ordering::SeqCst));
        assert!(second_done.load(Ordering::SeqCst));
    }

    #[test]
    fn a_timeout_fails_closed_and_stays_failed() {
        let running = Arc::new(AtomicBool::new(true));
        let executor = executor_with(vec![blocked_until(running.clone())]);
        assert!(!stop(&executor, Duration::ZERO));
        assert_eq!(executor.lock().stopped, LoaderShutdown::Failed);
        let started = Stamp::now();
        assert!(!stop(&executor, Duration::from_secs(30)));
        assert!(started.elapsed() < Duration::from_secs(1));
        running.store(false, Ordering::SeqCst);
    }

    #[test]
    fn a_panicked_loader_fails_closed() {
        let executor = executor_with(vec![std::thread::spawn(|| panic!("loader failed"))]);
        assert!(!stop(&executor, Duration::from_secs(5)));
        assert_eq!(executor.lock().stopped, LoaderShutdown::Failed);
        assert!(!stop(&executor, Duration::from_secs(5)));
    }

    #[test]
    fn concurrent_shutdowns_join_once_and_agree() {
        let (done, finished) = flag();
        let executor = Arc::new(executor_with(vec![finishing_after(
            Duration::from_millis(50),
            done,
        )]));
        let callers: Vec<_> = (0..2)
            .map(|_| {
                let executor = executor.clone();
                std::thread::spawn(move || stop(&executor, Duration::from_secs(5)))
            })
            .collect();
        for caller in callers {
            assert!(caller.join().unwrap());
        }
        assert!(finished.load(Ordering::SeqCst));
        assert_eq!(executor.lock().stopped, LoaderShutdown::Done);
    }

    #[test]
    fn a_concurrent_timeout_keeps_a_joined_loader_failed() {
        let (done, _) = flag();
        let executor = Arc::new(executor_with(vec![finishing_after(
            Duration::from_millis(100),
            done,
        )]));
        let joiner = {
            let executor = executor.clone();
            std::thread::spawn(move || stop(&executor, Duration::from_secs(5)))
        };
        std::thread::sleep(Duration::from_millis(20));
        assert!(!stop(&executor, Duration::ZERO));
        assert!(!joiner.join().unwrap());
        assert_eq!(executor.lock().stopped, LoaderShutdown::Failed);
    }

    #[test]
    fn a_closed_executor_starts_no_load_and_keeps_its_stage_off() {
        let mut executor = executor_with(Vec::new()).into_inner();
        executor.closed = true;
        executor.start();
        assert!(matches!(executor.stage, Stage::Off));
        assert!(executor.handles.is_empty());
    }
}
