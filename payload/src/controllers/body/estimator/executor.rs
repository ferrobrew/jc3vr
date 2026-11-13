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
    time::{Duration, Instant},
};

use body_estimator::{
    EgoPoser, WeightsError,
    egoposer::gpu::{GpuEgoPoser, GpuError, Readback, WeightPrecision},
};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};

use crate::{
    config::Config,
    controllers::{
        body::estimator::{
            Reading,
            config::{DEFAULT_WEIGHTS_FILE, WEIGHTS_SCRIPT},
            producer, publish, withdraw,
        },
        roomscale::posture::EffectivePosture,
    },
};

/// Why the estimator could not run.
#[derive(Debug)]
pub enum EstimatorError {
    /// Neither the settings nor the payload's own location name the weights' file.
    NoWeightsPath,
    /// The weights' file is not there.
    MissingWeights { path: PathBuf },
    /// The weights' file could not be loaded.
    Weights(WeightsError),
    /// The thread that loads the weights could not start.
    LoaderThread(io::Error),
    /// The loader thread ended without a result.
    LoaderLost,
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
            EstimatorError::LoaderLost => {
                write!(f, "the weights' loader thread ended without a result")
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
            | EstimatorError::LoaderLost => None,
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
    let config = Config::lock_query(|c| c.controllers.body.estimator.clone());
    let mut executor = EXECUTOR.lock();
    if !config.enabled {
        if !matches!(executor.stage, Stage::Off) {
            tracing::info!(target: "body", "body estimator: off");
            executor.stage = Stage::Off;
            executor.in_flight.clear();
        }
        return;
    }
    let path = config.weights(crate::module::get_path());
    let now = Instant::now();
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
            let started = Instant::now();
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

/// The executor's state, for the debug UI.
pub fn status() -> ExecutorStatus {
    let enabled = Config::lock_query(|c| c.controllers.body.estimator.enabled);
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
    window_end: Instant,
    posture: EffectivePosture,
    submitted_at: Instant,
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
}

impl Executor {
    /// Start or load as the stage needs. `wanted` is whether a window arrived this frame:
    /// nothing loads before the first.
    fn advance(&mut self, device: &ID3D11Device, wanted: bool) {
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
                Err(TryRecvError::Disconnected) => self.fail(EstimatorError::LoaderLost),
            },
            _ => {}
        }
    }

    /// Look for the weights and start loading them. Weights that are not there are a broken
    /// installation, so they panic rather than leave the estimator quietly off.
    fn start(&mut self) {
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
            Ok(_) => self.stage = Stage::Loading(receiver),
            Err(error) => self.fail(EstimatorError::LoaderThread(error)),
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
    fn finish(&mut self, readback: Readback, now: Instant) {
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
});
