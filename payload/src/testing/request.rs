//! The request file: a TOML file a script drops into the configuration directory to start or stop
//! the test tools without the debug UI or a key. The game thread looks for it once a second, and
//! claims, reads, deletes, and acts on it.
//!
//! ```toml
//! command = "scenario"  # record, stop_recording, replay, stop_replay, scenario, or stop_scenario
//! path = "climb.toml"   # replay and scenario: relative to the scenario folder, or absolute
//! start = "marker"      # replay: marker, bookmark, or in_place
//! pacing = "real_time"  # replay: real_time or frame_locked
//! ```

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::Deserialize;

use crate::{
    config::Config,
    testing::{
        replay::{self, Pacing, ReplayOptions, StartMode},
        scenario,
        text::{self, TextError},
        visual,
    },
};

/// The request file's name in the configuration directory.
pub(crate) const FILE_NAME: &str = "test_request.toml";

/// Look for the request file and act on it, at most once every [`POLL_INTERVAL`]. Game thread.
pub(crate) fn poll() {
    {
        let mut last = LAST_POLL.lock();
        if last.is_some_and(|at| at.elapsed() < POLL_INTERVAL) {
            return;
        }
        *last = Some(Instant::now());
    }
    let (enabled, watched) = Config::lock_query(|c| (c.testing.enabled, c.testing.request_file));
    if !enabled || !watched {
        return;
    }
    let Some(path) = crate::config::paths::file(FILE_NAME) else {
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

/// How often the request file is looked for.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

static LAST_POLL: Mutex<Option<Instant>> = Mutex::new(None);

/// A request, read.
#[derive(Debug, Clone, PartialEq)]
enum Request {
    Record,
    StopRecording,
    Replay {
        path: PathBuf,
        options: ReplayOptions,
    },
    StopReplay,
    Scenario(PathBuf),
    StopScenario,
    Screenshot {
        id: crate::screenshot::harness::RequestId,
        motion: Option<u32>,
        water_reflection: Option<crate::screenshot::harness::WaterReflection>,
        water_inputs: bool,
        sun_inputs: bool,
        resolve_inputs: bool,
        sun_depth_clip: Option<crate::screenshot::harness::SunDepthClip>,
    },
    Visual(visual::Request),
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
    /// The command needs a path and has none.
    NoPath(Command),
    /// The scenario folder is unknown, so a relative path cannot be resolved.
    NoFolder,
    InvalidId(&'static str),
    InvalidPose(visual::PoseError),
    InvalidMotion(visual::MotionError),
    InvalidEnvironment(visual::EnvironmentError),
    PoseOnCommand(Command),
    MotionOnCommand(Command),
    EnvironmentOnCommand(Command),
    VisualOverridesOnCommand(Command),
    ShaderIdentityOnCommand(Command),
    MotionCaptureOnCommand(Command),
    WaterReflectionOnCommand(Command),
    WaterReflectionNeedsInputs,
    WaterInputsOnCommand(Command),
    SunInputsOnCommand(Command),
    ResolveInputsOnCommand(Command),
    SunDepthClipOnCommand(Command),
    SunDepthClipNeedsSunInputs,
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
            RequestErrorKind::NoPath(command) => write!(f, "{command:?} needs a path"),
            RequestErrorKind::InvalidId(reason) => write!(f, "invalid request id: {reason}"),
            RequestErrorKind::InvalidPose(error) => write!(f, "{error}"),
            RequestErrorKind::InvalidMotion(error) => write!(f, "{error}"),
            RequestErrorKind::InvalidEnvironment(error) => write!(f, "{error}"),
            RequestErrorKind::PoseOnCommand(command) => {
                write!(f, "only visual_configure accepts a pose, not {command:?}")
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
            RequestErrorKind::WaterReflectionOnCommand(command) => write!(
                f,
                "only screenshot accepts water_reflection, not {command:?}"
            ),
            RequestErrorKind::WaterReflectionNeedsInputs => {
                write!(f, "water_reflection requires water_inputs")
            }
            RequestErrorKind::WaterInputsOnCommand(command) => {
                write!(f, "only screenshot accepts water_inputs, not {command:?}")
            }
            RequestErrorKind::SunDepthClipOnCommand(command) => {
                write!(f, "only screenshot accepts sun_depth_clip, not {command:?}")
            }
            RequestErrorKind::SunDepthClipNeedsSunInputs => {
                write!(f, "sun_depth_clip requires sun_inputs")
            }
            RequestErrorKind::ResolveInputsOnCommand(command) => {
                write!(f, "only screenshot accepts resolve_inputs, not {command:?}")
            }
            RequestErrorKind::SunInputsOnCommand(command) => {
                write!(f, "only screenshot accepts sun_inputs, not {command:?}")
            }
            RequestErrorKind::NoMode => write!(f, "visual_configure needs a mode"),
            RequestErrorKind::InvalidMode => {
                write!(f, "visual_configure does not accept the native mode")
            }
            RequestErrorKind::InvalidWarmup => {
                write!(f, "visual_configure warmup_frames exceeds 300")
            }
            RequestErrorKind::NoFolder => {
                write!(
                    f,
                    "the scenario folder is unknown, so the path must be absolute"
                )
            }
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Command {
    VisualConfigure,
    VisualEnd,
    Screenshot,
    Record,
    StopRecording,
    Replay,
    StopReplay,
    Scenario,
    StopScenario,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestFile {
    command: Command,
    id: Option<String>,
    path: Option<String>,
    #[serde(default)]
    start: StartMode,
    #[serde(default)]
    pacing: Pacing,
    mode: Option<visual::RenderMode>,
    warmup_frames: Option<u32>,
    pose: Option<visual::PoseTable>,
    motion: Option<visual::MotionTable>,
    environment: Option<visual::EnvironmentTable>,
    shader_identity: Option<bool>,
    visual_overrides: Option<visual::VisualOverrides>,
    motion_capture: Option<MotionCaptureTable>,
    water_reflection: Option<crate::screenshot::harness::WaterReflection>,
    #[serde(default)]
    water_inputs: bool,
    #[serde(default)]
    sun_inputs: bool,
    #[serde(default)]
    resolve_inputs: bool,
    sun_depth_clip: Option<crate::screenshot::harness::SunDepthClip>,
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
    if let Some(path) = &file.path {
        text::string("path", path).map_err(|e| error(RequestErrorKind::Read(e)))?;
    }
    let resolve = |command| -> Result<PathBuf, RequestError> {
        let given = file
            .path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .ok_or_else(|| error(RequestErrorKind::NoPath(command)))?;
        let given = PathBuf::from(given);
        if given.is_absolute() {
            return Ok(given);
        }
        scenario::folder()
            .map(|folder| folder.join(&given))
            .ok_or_else(|| error(RequestErrorKind::NoFolder))
    };
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
    if file.shader_identity.is_some() && file.command != Command::VisualConfigure {
        return Err(error(RequestErrorKind::ShaderIdentityOnCommand(
            file.command,
        )));
    }
    if file.visual_overrides.is_some() && file.command != Command::VisualConfigure {
        return Err(error(RequestErrorKind::VisualOverridesOnCommand(
            file.command,
        )));
    }
    let motion_capture = match file.motion_capture {
        Some(table) => {
            if file.command != Command::Screenshot {
                return Err(error(RequestErrorKind::MotionCaptureOnCommand(
                    file.command,
                )));
            }
            Some(table.phase)
        }
        None => None,
    };
    if file.water_reflection.is_some() && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::WaterReflectionOnCommand(
            file.command,
        )));
    }
    if file.water_reflection.is_some() && !file.water_inputs {
        return Err(error(RequestErrorKind::WaterReflectionNeedsInputs));
    }
    if file.water_inputs && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::WaterInputsOnCommand(file.command)));
    }
    if file.sun_inputs && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::SunInputsOnCommand(file.command)));
    }
    if file.resolve_inputs && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::ResolveInputsOnCommand(
            file.command,
        )));
    }
    if file.sun_depth_clip.is_some() && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::SunDepthClipOnCommand(file.command)));
    }
    if file.sun_depth_clip.is_some() && !file.sun_inputs {
        return Err(error(RequestErrorKind::SunDepthClipNeedsSunInputs));
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
                visual::Request::Configure {
                    id,
                    mode,
                    warmup_frames,
                    pose,
                    motion,
                    environment,
                    shader_identity: file.shader_identity.unwrap_or(false),
                    visual_overrides: file.visual_overrides,
                }
            })
        }
        Command::Screenshot => Request::Screenshot {
            id: crate::screenshot::harness::RequestId::parse(
                file.id.as_deref().unwrap_or_default(),
            )
            .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?,
            motion: motion_capture,
            water_reflection: file.water_reflection,
            water_inputs: file.water_inputs,
            sun_inputs: file.sun_inputs,
            resolve_inputs: file.resolve_inputs,
            sun_depth_clip: file.sun_depth_clip,
        },
        Command::Record => Request::Record,
        Command::StopRecording => Request::StopRecording,
        Command::Replay => Request::Replay {
            path: resolve(Command::Replay)?,
            options: ReplayOptions {
                start: file.start,
                pacing: file.pacing,
            },
        },
        Command::StopReplay => Request::StopReplay,
        Command::Scenario => Request::Scenario(resolve(Command::Scenario)?),
        Command::StopScenario => Request::StopScenario,
    })
}

fn act(request: Request) {
    tracing::info!(?request, "testing: request file");
    match request {
        Request::Visual(request) => visual::enqueue(request),
        Request::Screenshot {
            id,
            motion,
            water_reflection,
            water_inputs,
            sun_inputs,
            resolve_inputs,
            sun_depth_clip,
        } => {
            let options = crate::screenshot::harness::CaptureRequest {
                water_reflection,
                water_inputs,
                sun_inputs,
                resolve_inputs,
                sun_depth_clip,
            };
            match motion {
                Some(phase) => crate::screenshot::harness::request_motion(id, phase, options),
                None => crate::screenshot::harness::request(id, options),
            }
        }
        Request::Record => crate::recorder::tracking::request_start(),
        Request::StopRecording => crate::recorder::tracking::request_stop(),
        Request::Replay { path, options } => {
            replay::start(path, options);
        }
        Request::StopReplay => replay::stop(),
        Request::Scenario(path) => scenario::start(path),
        Request::StopScenario => scenario::stop(),
    }
}

#[cfg(test)]
mod tests;
