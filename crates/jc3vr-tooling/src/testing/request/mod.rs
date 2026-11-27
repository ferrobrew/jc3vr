//! The request file: a TOML file a script drops into the configuration directory to start or stop
//! the test tools without the debug UI or a key. The game thread looks for it once a second (every
//! frame in a capture harness session), and claims, reads, deletes, and acts on it.
//!
//! ```toml
//! command = "record"  # record or stop_recording, or a capture harness command
//! ```

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use jc3vr_capture::{
    screenshot::{
        harness,
        harness::{
            CaptureRequest, RequestId,
            snapshot::{SnapshotError, SnapshotRequest, SnapshotTable},
        },
    },
    testing::{
        benchmark::{BenchmarkError, BenchmarkRequest, BenchmarkTable},
        visual,
    },
};
use parking_lot::Mutex;
use serde::Deserialize;

use crate::testing::{
    TESTING_CONFIG,
    stage::{self, StageRequest, StageTable},
    text::{self, TextError},
};

/// The request file's name in the configuration directory.
pub const FILE_NAME: &str = "test_request.toml";

/// Look for the request file and act on it, at most once every [`POLL_INTERVAL`], or every frame
/// in a capture harness session. Game thread.
pub(crate) fn poll() {
    {
        let mut last = LAST_POLL.lock();
        let interval = if harness_session() {
            Duration::ZERO
        } else {
            POLL_INTERVAL
        };
        if last.is_some_and(|at| at.elapsed() < interval) {
            return;
        }
        *last = Some(Instant::now());
    }
    let (enabled, watched) = TESTING_CONFIG.query(|c| (c.enabled, c.request_file));
    if !enabled || !watched {
        return;
    }
    let Some(path) = jc3vr_core::config::paths::file(FILE_NAME) else {
        return;
    };
    match take(&path) {
        Ok(Some(text)) => match parse(&path, &text) {
            Ok(request) => act(request),
            Err(error) => tracing::warn!("{error}"),
        },
        Ok(None) => {}
        Err(error) => tracing::warn!("{error}"),
    }
}

/// How often the request file is looked for outside a capture harness session.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Whether the game runs in a capture harness session (`tools/rendering`), whose requests come
/// one after another and should not each wait out [`POLL_INTERVAL`]. A file check is cheap next to
/// a frame.
fn harness_session() -> bool {
    static SESSION: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SESSION.get_or_init(|| std::env::var_os("JC3VR_HARNESS_TOKEN").is_some())
}

static LAST_POLL: Mutex<Option<Instant>> = Mutex::new(None);

/// A request, read.
#[derive(Debug, Clone, PartialEq)]
enum Request {
    Record,
    StopRecording,
    Screenshot {
        id: RequestId,
        motion: Option<u32>,
        options: CaptureRequest,
    },
    Visual(visual::Request),
    Stage(StageRequest),
    Snapshot(SnapshotRequest),
    SnapshotRelease(RequestId),
    Benchmark(BenchmarkRequest),
}

/// Why a request file could not be used.
#[derive(Debug)]
struct RequestError {
    path: PathBuf,
    kind: RequestErrorKind,
}

#[derive(Debug)]
enum RequestErrorKind {
    Read(TextError),
    Claim(std::io::Error),
    Remove(std::io::Error),
    Parse(toml::de::Error),
    InvalidId(&'static str),
    InvalidPose(visual::PoseError),
    InvalidMotion(visual::MotionError),
    InvalidEnvironment(visual::EnvironmentError),
    InvalidStage(stage::StageError),
    InvalidSnapshot(SnapshotError),
    SnapshotOnCommand(Command),
    NoSnapshot,
    InvalidBenchmark(BenchmarkError),
    BenchmarkOnCommand(Command),
    NoBenchmark,
    StageOnCommand(Command),
    NoStage,
    PoseOnCommand(Command),
    AnchorOnCommand(Command),
    MotionOnCommand(Command),
    EnvironmentOnCommand(Command),
    VisualOverridesOnCommand(Command),
    InvalidVisualOverrides(visual::OverrideError),
    ShaderIdentityOnCommand(Command),
    MotionCaptureOnCommand(Command),
    CaptureOptionsOnCommand(Command),
    UnknownKeys(Vec<String>),
    MotionWithPose,
    NoMode,
    InvalidMode,
    InvalidWarmup,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "testing: request file {}: ", self.path.display())?;
        match &self.kind {
            RequestErrorKind::Read(e) => write!(f, "could not read it: {e}"),
            RequestErrorKind::Claim(e) => write!(f, "could not claim it: {e}"),
            RequestErrorKind::Remove(e) => {
                write!(f, "could not remove it, so it is not acted on: {e}")
            }
            RequestErrorKind::Parse(e) => write!(f, "could not parse it: {e}"),
            RequestErrorKind::InvalidId(reason) => write!(f, "invalid request id: {reason}"),
            RequestErrorKind::InvalidPose(error) => write!(f, "{error}"),
            RequestErrorKind::InvalidMotion(error) => write!(f, "{error}"),
            RequestErrorKind::InvalidEnvironment(error) => write!(f, "{error}"),
            RequestErrorKind::InvalidStage(error) => write!(f, "{error}"),
            RequestErrorKind::InvalidSnapshot(error) => write!(f, "{error}"),
            RequestErrorKind::SnapshotOnCommand(command) => {
                write!(f, "only snapshot accepts a snapshot table, not {command:?}")
            }
            RequestErrorKind::NoSnapshot => write!(f, "snapshot needs a snapshot table"),
            RequestErrorKind::InvalidBenchmark(error) => write!(f, "{error}"),
            RequestErrorKind::BenchmarkOnCommand(command) => {
                write!(
                    f,
                    "only benchmark accepts a benchmark table, not {command:?}"
                )
            }
            RequestErrorKind::NoBenchmark => write!(f, "benchmark needs a benchmark table"),
            RequestErrorKind::StageOnCommand(command) => {
                write!(
                    f,
                    "only visual_stage accepts a stage table, not {command:?}"
                )
            }
            RequestErrorKind::NoStage => write!(f, "visual_stage needs a stage table"),
            RequestErrorKind::PoseOnCommand(command) => {
                write!(f, "only visual_configure accepts a pose, not {command:?}")
            }
            RequestErrorKind::AnchorOnCommand(command) => {
                write!(
                    f,
                    "only visual_configure accepts an anchor, not {command:?}"
                )
            }
            RequestErrorKind::MotionOnCommand(command) => {
                write!(
                    f,
                    "only visual_configure accepts a motion table, not {command:?}"
                )
            }
            RequestErrorKind::EnvironmentOnCommand(command) => {
                write!(
                    f,
                    "only visual_configure accepts an environment table, not {command:?}"
                )
            }
            RequestErrorKind::ShaderIdentityOnCommand(command) => write!(
                f,
                "only visual_configure accepts shader_identity, not {command:?}"
            ),
            RequestErrorKind::VisualOverridesOnCommand(command) => {
                write!(
                    f,
                    "only visual_configure accepts visual_overrides, not {command:?}"
                )
            }
            RequestErrorKind::InvalidVisualOverrides(error) => write!(f, "{error}"),
            RequestErrorKind::MotionCaptureOnCommand(command) => {
                write!(
                    f,
                    "only screenshot accepts a motion_capture table, not {command:?}"
                )
            }
            RequestErrorKind::MotionWithPose => {
                write!(
                    f,
                    "visual_configure accepts either a pose or a motion table, not both"
                )
            }
            RequestErrorKind::CaptureOptionsOnCommand(command) => {
                write!(
                    f,
                    "only screenshot accepts screenshot options, not {command:?}"
                )
            }
            RequestErrorKind::UnknownKeys(keys) => {
                write!(f, "unknown keys: {}", keys.join(", "))
            }
            RequestErrorKind::NoMode => write!(f, "visual_configure needs a mode"),
            RequestErrorKind::InvalidMode => {
                write!(f, "visual_configure does not accept the native mode")
            }
            RequestErrorKind::InvalidWarmup => {
                write!(f, "visual_configure warmup_frames exceeds 300")
            }
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Command {
    VisualConfigure,
    VisualEnd,
    VisualStage,
    Screenshot,
    Snapshot,
    SnapshotRelease,
    Benchmark,
    Record,
    StopRecording,
}

#[derive(Deserialize)]
struct RequestFile {
    command: Command,
    id: Option<String>,
    mode: Option<visual::RenderMode>,
    warmup_frames: Option<u32>,
    pose: Option<visual::PoseTable>,
    anchor: Option<visual::AnchorTable>,
    motion: Option<visual::MotionTable>,
    environment: Option<visual::EnvironmentTable>,
    stage: Option<StageTable>,
    shader_identity: Option<bool>,
    visual_overrides: Option<visual::VisualOverridesTable>,
    motion_capture: Option<MotionCaptureTable>,
    snapshot: Option<SnapshotTable>,
    benchmark: Option<BenchmarkTable>,
    #[serde(flatten)]
    capture: CaptureRequest,
    /// Whatever no field above claims. The struct cannot deny unknown keys itself while it
    /// flattens the screenshot options, so [`parse`] refuses a request that leaves any here.
    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionCaptureTable {
    phase: u32,
}

/// Claim before reading so cancellation and replacement cannot redirect the final unlink.
/// The game thread is the sole consumer; an interrupted claim is discarded, never replayed.
fn take(path: &Path) -> Result<Option<String>, RequestError> {
    claim(path)?.map(ClaimedRequest::read).transpose()
}

struct ClaimedRequest {
    path: PathBuf,
}

impl ClaimedRequest {
    fn read(self) -> Result<String, RequestError> {
        let error = |kind| RequestError {
            path: self.path.clone(),
            kind,
        };
        let text = text::read(&self.path, text::REQUEST_BYTES)
            .map_err(|e| error(RequestErrorKind::Read(e)));
        let removed =
            std::fs::remove_file(&self.path).map_err(|e| error(RequestErrorKind::Remove(e)));
        removed?;
        text
    }
}

fn claim(path: &Path) -> Result<Option<ClaimedRequest>, RequestError> {
    let claimed = path.with_extension("toml.claimed");
    // One fixed claim bounds stale files across restarts. Only the consumer touches this path.
    match std::fs::remove_file(&claimed) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(RequestError {
                path: claimed,
                kind: RequestErrorKind::Remove(error),
            });
        }
    }
    match std::fs::rename(path, &claimed) {
        Ok(()) => Ok(Some(ClaimedRequest { path: claimed })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(RequestError {
            path: path.to_owned(),
            kind: RequestErrorKind::Claim(error),
        }),
    }
}

fn parse(path: &Path, text: &str) -> Result<Request, RequestError> {
    let error = |kind| RequestError {
        path: path.to_owned(),
        kind,
    };
    text::limit("request file", text.len(), text::REQUEST_BYTES)
        .map_err(|e| error(RequestErrorKind::Read(e)))?;
    let file: RequestFile = toml::from_str(text).map_err(|e| error(RequestErrorKind::Parse(e)))?;
    if !file.unknown.is_empty() {
        return Err(error(RequestErrorKind::UnknownKeys(
            file.unknown.into_keys().collect(),
        )));
    }
    let pose = match file.pose {
        Some(table) => {
            if file.command != Command::VisualConfigure {
                return Err(error(RequestErrorKind::PoseOnCommand(file.command)));
            }
            Some(
                visual::PoseRequest::try_from(table)
                    .map_err(|reason| error(RequestErrorKind::InvalidPose(reason)))?,
            )
        }
        None => None,
    };
    let anchor = match file.anchor {
        Some(table) => {
            if file.command != Command::VisualConfigure {
                return Err(error(RequestErrorKind::AnchorOnCommand(file.command)));
            }
            Some(Box::new(visual::AnchorPin::try_from(table).map_err(
                |reason| error(RequestErrorKind::InvalidPose(reason)),
            )?))
        }
        None => None,
    };
    let motion = match file.motion {
        Some(table) => {
            if file.command != Command::VisualConfigure {
                return Err(error(RequestErrorKind::MotionOnCommand(file.command)));
            }
            Some(
                visual::MotionRequest::try_from(table)
                    .map_err(|reason| error(RequestErrorKind::InvalidMotion(reason)))?,
            )
        }
        None => None,
    };
    if pose.is_some() && motion.is_some() {
        return Err(error(RequestErrorKind::MotionWithPose));
    }
    let environment = match file.environment {
        Some(table) => {
            if file.command != Command::VisualConfigure {
                return Err(error(RequestErrorKind::EnvironmentOnCommand(file.command)));
            }
            Some(
                visual::EnvironmentRequest::try_from(table)
                    .map_err(|e| error(RequestErrorKind::InvalidEnvironment(e)))?,
            )
        }
        None => None,
    };
    if file.stage.is_some() && file.command != Command::VisualStage {
        return Err(error(RequestErrorKind::StageOnCommand(file.command)));
    }
    if file.shader_identity.is_some() && file.command != Command::VisualConfigure {
        return Err(error(RequestErrorKind::ShaderIdentityOnCommand(
            file.command,
        )));
    }
    let visual_overrides = match file.visual_overrides {
        Some(table) => {
            if file.command != Command::VisualConfigure {
                return Err(error(RequestErrorKind::VisualOverridesOnCommand(
                    file.command,
                )));
            }
            Some(
                visual::VisualOverrides::try_from(table)
                    .map_err(|e| error(RequestErrorKind::InvalidVisualOverrides(e)))?,
            )
        }
        None => None,
    };
    if file.snapshot.is_some() && file.command != Command::Snapshot {
        return Err(error(RequestErrorKind::SnapshotOnCommand(file.command)));
    }
    if file.benchmark.is_some() && file.command != Command::Benchmark {
        return Err(error(RequestErrorKind::BenchmarkOnCommand(file.command)));
    }
    let motion_capture = match file.motion_capture {
        Some(table) => {
            if !matches!(file.command, Command::Screenshot | Command::Snapshot) {
                return Err(error(RequestErrorKind::MotionCaptureOnCommand(
                    file.command,
                )));
            }
            Some(table.phase)
        }
        None => None,
    };
    if file.capture != CaptureRequest::default() && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::CaptureOptionsOnCommand(
            file.command,
        )));
    }
    Ok(match file.command {
        Command::VisualConfigure | Command::VisualEnd => {
            let id = visual::RequestId::parse(file.id.as_deref().unwrap_or_default())
                .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?;
            Request::Visual(if file.command == Command::VisualEnd {
                visual::Request::End { id }
            } else {
                let mode = file.mode.ok_or_else(|| error(RequestErrorKind::NoMode))?;
                if mode == visual::RenderMode::Native {
                    return Err(error(RequestErrorKind::InvalidMode));
                }
                let warmup_frames = file.warmup_frames.unwrap_or(30);
                if warmup_frames > 300 {
                    return Err(error(RequestErrorKind::InvalidWarmup));
                }
                if let Some(overrides) = &visual_overrides {
                    overrides
                        .validate_for(mode)
                        .map_err(|e| error(RequestErrorKind::InvalidVisualOverrides(e)))?;
                }
                visual::Request::Configure {
                    id,
                    mode,
                    warmup_frames,
                    pose,
                    anchor,
                    motion,
                    environment,
                    shader_identity: file.shader_identity.unwrap_or(false),
                    visual_overrides: visual_overrides.map(Box::new),
                }
            })
        }
        Command::VisualStage => {
            let id = visual::RequestId::parse(file.id.as_deref().unwrap_or_default())
                .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?;
            let table = file.stage.ok_or_else(|| error(RequestErrorKind::NoStage))?;
            Request::Stage(
                StageRequest::new(id, table)
                    .map_err(|e| error(RequestErrorKind::InvalidStage(e)))?,
            )
        }
        Command::Snapshot => {
            let id = RequestId::parse(file.id.as_deref().unwrap_or_default())
                .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?;
            let table = file
                .snapshot
                .ok_or_else(|| error(RequestErrorKind::NoSnapshot))?;
            Request::Snapshot(
                SnapshotRequest::parse(id, motion_capture, table)
                    .map_err(|e| error(RequestErrorKind::InvalidSnapshot(e)))?,
            )
        }
        Command::Benchmark => {
            let id = RequestId::parse(file.id.as_deref().unwrap_or_default())
                .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?;
            let table = file
                .benchmark
                .ok_or_else(|| error(RequestErrorKind::NoBenchmark))?;
            Request::Benchmark(
                BenchmarkRequest::parse(id, table)
                    .map_err(|e| error(RequestErrorKind::InvalidBenchmark(e)))?,
            )
        }
        Command::SnapshotRelease => Request::SnapshotRelease(
            RequestId::parse(file.id.as_deref().unwrap_or_default())
                .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?,
        ),
        Command::Screenshot => Request::Screenshot {
            id: RequestId::parse(file.id.as_deref().unwrap_or_default())
                .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?,
            motion: motion_capture,
            options: file.capture,
        },
        Command::Record => Request::Record,
        Command::StopRecording => Request::StopRecording,
    })
}

fn act(request: Request) {
    tracing::info!(?request, "testing: request file");
    match request {
        Request::Visual(request) => visual::enqueue(request),
        Request::Stage(request) => stage::enqueue(request),
        Request::Snapshot(request) => harness::snapshot::request(request),
        Request::SnapshotRelease(id) => harness::snapshot::release(id),
        Request::Benchmark(request) => jc3vr_capture::testing::benchmark::request(request),
        Request::Screenshot {
            id,
            motion,
            options,
        } => match motion {
            Some(phase) => harness::request_motion(id, phase, options),
            None => harness::request(id, options),
        },
        Request::Record => jc3vr_view::recorder::tracking::request_start(),
        Request::StopRecording => jc3vr_view::recorder::tracking::request_stop(),
    }
}

#[cfg(test)]
mod tests;
