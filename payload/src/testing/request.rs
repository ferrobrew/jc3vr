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
    MotionCaptureOnCommand(Command),
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
    motion_capture: Option<MotionCaptureTable>,
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
                }
            })
        }
        Command::Screenshot => Request::Screenshot {
            id: crate::screenshot::harness::RequestId::parse(
                file.id.as_deref().unwrap_or_default(),
            )
            .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?,
            motion: motion_capture,
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
        Request::Screenshot { id, motion } => match motion {
            Some(phase) => crate::screenshot::harness::request_motion(id, phase),
            None => crate::screenshot::harness::request(id),
        },
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
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use pretty_assertions::assert_eq;

    use super::*;

    struct RequestDirectory(PathBuf);

    impl RequestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "jc3vr-request-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn request(&self) -> PathBuf {
            self.0.join(FILE_NAME)
        }
    }

    impl Drop for RequestDirectory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn a_claimed_request_cannot_remove_the_replacement_restoration() {
        let directory = RequestDirectory::new();
        let path = directory.request();
        std::fs::write(&path, "command = 'record'").unwrap();
        let claimed = claim(&path).unwrap().unwrap();
        assert!(!path.exists());
        let restoration = "command = 'visual_end'\nid = 'suite_end'";
        std::fs::write(&path, restoration).unwrap();
        assert_eq!(claimed.read().unwrap(), "command = 'record'");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), restoration);
        assert_eq!(take(&path).unwrap().as_deref(), Some(restoration));
        assert!(take(&path).unwrap().is_none());
    }

    #[test]
    fn cancellation_before_claim_preserves_the_restoration_request() {
        let directory = RequestDirectory::new();
        let path = directory.request();
        std::fs::write(&path, "command = 'record'").unwrap();
        std::fs::rename(&path, path.with_extension("cancelled")).unwrap();
        assert!(take(&path).unwrap().is_none());
        let restoration = "command = 'visual_end'\nid = 'suite_end'";
        std::fs::write(&path, restoration).unwrap();
        assert_eq!(take(&path).unwrap().as_deref(), Some(restoration));
    }

    #[test]
    fn stale_claims_are_discarded_without_replaying_and_do_not_accumulate() {
        let directory = RequestDirectory::new();
        let path = directory.request();
        let stale = path.with_extension("toml.claimed");
        std::fs::write(&stale, "command = 'record'").unwrap();
        assert!(take(&path).unwrap().is_none());
        assert!(!stale.exists());
        std::fs::write(&stale, "command = 'record'").unwrap();
        std::fs::write(&path, "command = 'stop_recording'").unwrap();
        assert_eq!(
            take(&path).unwrap().as_deref(),
            Some("command = 'stop_recording'")
        );
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 0);
    }

    #[test]
    fn unreadable_claims_are_removed_without_touching_new_requests() {
        let directory = RequestDirectory::new();
        let path = directory.request();
        for bytes in [vec![0xff], vec![b'x'; text::REQUEST_BYTES + 1]] {
            std::fs::write(&path, bytes).unwrap();
            let claimed = claim(&path).unwrap().unwrap();
            std::fs::write(&path, "command = 'stop_recording'").unwrap();
            assert!(matches!(
                claimed.read().unwrap_err().kind,
                RequestErrorKind::Read(_)
            ));
            assert_eq!(
                take(&path).unwrap().as_deref(),
                Some("command = 'stop_recording'")
            );
        }
    }

    #[test]
    fn blocked_stale_claim_cleanup_leaves_the_pending_request_intact() {
        let directory = RequestDirectory::new();
        let path = directory.request();
        std::fs::create_dir(path.with_extension("toml.claimed")).unwrap();
        std::fs::write(&path, "command = 'stop_recording'").unwrap();
        assert!(matches!(
            take(&path).unwrap_err().kind,
            RequestErrorKind::Remove(_)
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "command = 'stop_recording'"
        );
    }

    fn read(text: &str) -> Result<Request, RequestErrorKind> {
        parse(Path::new("C:/config/test_request.toml"), text).map_err(|e| e.kind)
    }

    #[test]
    fn screenshot_ids_are_required_and_path_safe() {
        assert!(matches!(
            read("command = 'screenshot'\nid = 'safe-ascii-id'"),
            Ok(Request::Screenshot { motion: None, .. })
        ));
        for id in [
            "",
            "../escape",
            "a/b",
            "a\\b",
            "é",
            "has space",
            &"x".repeat(65),
        ] {
            let request = format!("command = 'screenshot'\nid = '{id}'");
            assert!(matches!(
                read(&request),
                Err(RequestErrorKind::InvalidId(_))
            ));
        }
        assert!(matches!(
            read("command = 'screenshot'"),
            Err(RequestErrorKind::InvalidId(_))
        ));
    }

    #[test]
    fn visual_requests_validate_ids_modes_and_warmup() {
        for mode in [
            "left_only",
            "right_only",
            "multipass",
            "reverse_multipass",
            "single_pass",
        ] {
            let request =
                format!("command = 'visual_configure'\nid = 'configure-1'\nmode = '{mode}'");
            assert!(matches!(
                read(&request),
                Ok(Request::Visual(visual::Request::Configure {
                    warmup_frames: 30,
                    pose: None,
                    ..
                }))
            ));
        }
        assert!(matches!(
            read("command = 'visual_end'\nid = 'end-1'"),
            Ok(Request::Visual(visual::Request::End { .. }))
        ));
        for command in ["visual_configure", "visual_end"] {
            assert!(matches!(
                read(&format!("command = '{command}'\nid = '../escape'")),
                Err(RequestErrorKind::InvalidId(_))
            ));
        }
        assert!(matches!(
            read("command = 'visual_configure'\nid = 'missing-mode'"),
            Err(RequestErrorKind::NoMode)
        ));
        assert!(matches!(
            read("command = 'visual_configure'\nid = 'native'\nmode = 'native'"),
            Err(RequestErrorKind::InvalidMode)
        ));
        for frames in [0, 30, 300] {
            assert!(matches!(
                read(&format!(
                    "command = 'visual_configure'\nid = 'valid'\nmode = 'multipass'\nwarmup_frames = {frames}"
                )),
                Ok(Request::Visual(_))
            ));
        }
        assert!(matches!(
            read(
                "command = 'visual_configure'\nid = 'invalid'\nmode = 'multipass'\nwarmup_frames = 301"
            ),
            Err(RequestErrorKind::InvalidWarmup)
        ));
        assert!(matches!(
            read(
                "command = 'visual_configure'\nid = 'invalid'\nmode = 'multipass'\nwarmup_frames = -1"
            ),
            Err(RequestErrorKind::Parse(_))
        ));
    }

    #[test]
    fn visual_environment_tables_validate_hours_weather_and_commands() {
        let header = "command = 'visual_configure'\nid = 'configure-env'\nmode = 'multipass'\n";
        for hour in ["0.0", "13.5", "23.999"] {
            let text = format!("{header}[environment]\nhour = {hour}\nweather = 'sunny'");
            assert!(matches!(
                read(&text),
                Ok(Request::Visual(visual::Request::Configure {
                    environment: Some(_),
                    ..
                }))
            ));
        }
        for hour in ["nan", "inf", "-inf"] {
            let text = format!("{header}[environment]\nhour = {hour}\nweather = 'sunny'");
            assert!(matches!(
                read(&text),
                Err(RequestErrorKind::InvalidEnvironment(
                    visual::EnvironmentError::NonFiniteHour
                ))
            ));
        }
        for hour in ["-0.001", "24.0", "25.0"] {
            let text = format!("{header}[environment]\nhour = {hour}\nweather = 'sunny'");
            assert!(matches!(
                read(&text),
                Err(RequestErrorKind::InvalidEnvironment(
                    visual::EnvironmentError::HourOutOfRange
                ))
            ));
        }
        for text in [
            format!("{header}[environment]\nweather = 'sunny'"),
            format!("{header}[environment]\nhour = 13.5"),
            format!("{header}[environment]\nhour = 13.5\nweather = 'rain'"),
            format!("{header}[environment]\nhour = 13.5\nweather = 'sunny'\nday_cycle_speed = 0.0"),
        ] {
            assert!(matches!(read(&text), Err(RequestErrorKind::Parse(_))));
        }
        for (command, extra, expected) in [
            ("visual_end", "\nid = 'end-env'", Command::VisualEnd),
            ("record", "", Command::Record),
            ("screenshot", "\nid = 'shot'", Command::Screenshot),
            ("scenario", "\npath = 'climb.toml'", Command::Scenario),
        ] {
            let text = format!(
                "command = '{command}'{extra}\n[environment]\nhour = 13.5\nweather = 'sunny'"
            );
            assert!(
                matches!(
                    read(&text),
                    Err(RequestErrorKind::EnvironmentOnCommand(command)) if command == expected
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn visual_pose_tables_validate_shape_ids_and_angles() {
        let header = "command = 'visual_configure'\nid = 'configure-pose'\nmode = 'multipass'\n";
        for (yaw, pitch, roll) in [
            (0.0, 0.0, 0.0),
            (180.0, -180.0, 180.0),
            (-180.0, 180.0, -180.0),
            (30.0, -12.5, 4.25),
        ] {
            let text = format!(
                "{header}[pose]\ncase_id = 'sweep-1'\nyaw_deg = {yaw}\npitch_deg = {pitch}\nroll_deg = {roll}"
            );
            assert!(matches!(
                read(&text),
                Ok(Request::Visual(visual::Request::Configure {
                    pose: Some(_),
                    ..
                }))
            ));
        }
        assert!(matches!(
            read(header),
            Ok(Request::Visual(visual::Request::Configure {
                pose: None,
                ..
            }))
        ));
        for id in ["", "../escape", "a/b", "é", &"x".repeat(65)] {
            let text = format!(
                "{header}[pose]\ncase_id = '{id}'\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0"
            );
            assert!(matches!(
                read(&text),
                Err(RequestErrorKind::InvalidPose(
                    visual::PoseError::InvalidCaseId(_)
                ))
            ));
        }
        for (angle, expected) in [
            ("181.0", visual::PoseError::AngleOutOfRange("yaw")),
            ("-181.0", visual::PoseError::AngleOutOfRange("yaw")),
            ("nan", visual::PoseError::NonFiniteAngle("yaw")),
            ("inf", visual::PoseError::NonFiniteAngle("yaw")),
            ("-inf", visual::PoseError::NonFiniteAngle("yaw")),
        ] {
            let text = format!(
                "{header}[pose]\ncase_id = 'sweep-1'\nyaw_deg = {angle}\npitch_deg = 0.0\nroll_deg = 0.0"
            );
            assert!(
                matches!(read(&text), Err(RequestErrorKind::InvalidPose(error)) if error == expected)
            );
        }
        for text in [
            format!("{header}[pose]\ncase_id = 'sweep-1'\nyaw_deg = 0.0"),
            format!("{header}[pose]\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0"),
            format!(
                "{header}[pose]\ncase_id = 'sweep-1'\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0\nunknown = 1"
            ),
        ] {
            assert!(matches!(read(&text), Err(RequestErrorKind::Parse(_))));
        }
    }

    #[test]
    fn a_pose_table_is_only_accepted_by_visual_configure() {
        for (command, extra, expected) in [
            ("visual_end", "\nid = 'end-pose'", Command::VisualEnd),
            ("record", "", Command::Record),
            ("screenshot", "\nid = 'shot'", Command::Screenshot),
            ("scenario", "\npath = 'climb.toml'", Command::Scenario),
        ] {
            let text = format!(
                "command = '{command}'{extra}\n[pose]\ncase_id = 'sweep-1'\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0"
            );
            assert!(
                matches!(
                    read(&text),
                    Err(RequestErrorKind::PoseOnCommand(command)) if command == expected
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn oversized_requests_and_paths_never_reach_dispatch() {
        assert!(matches!(
            read(&"x".repeat(text::REQUEST_BYTES + 1)),
            Err(RequestErrorKind::Read(TextError::Limit { .. }))
        ));
        assert!(matches!(
            read(&format!(
                "command = 'replay'\npath = '{}'",
                "x".repeat(text::STRING_BYTES + 1)
            )),
            Err(RequestErrorKind::Read(TextError::Limit {
                field: "path",
                ..
            }))
        ));
    }

    #[test]
    fn a_replay_request_carries_its_options() {
        assert_eq!(
            read(
                "command = \"replay\"\npath = \"C:/recordings/climb.jc3trk\"\nstart = \
                 \"in_place\"\npacing = \"frame_locked\"\n"
            )
            .unwrap(),
            Request::Replay {
                path: PathBuf::from("C:/recordings/climb.jc3trk"),
                options: ReplayOptions {
                    start: StartMode::InPlace,
                    pacing: Pacing::FrameLocked,
                },
            }
        );
    }

    #[test]
    fn the_commands_without_a_path_read() {
        for (command, request) in [
            ("record", Request::Record),
            ("stop_recording", Request::StopRecording),
            ("stop_replay", Request::StopReplay),
            ("stop_scenario", Request::StopScenario),
        ] {
            assert_eq!(read(&format!("command = \"{command}\"")).unwrap(), request);
        }
    }

    #[test]
    fn a_scenario_without_a_path_or_an_unknown_command_is_refused() {
        assert!(matches!(
            read("command = \"scenario\"\npath = \" \""),
            Err(RequestErrorKind::NoPath(Command::Scenario))
        ));
        assert!(matches!(
            read("command = \"launch\""),
            Err(RequestErrorKind::Parse(_))
        ));
    }
    #[test]
    fn motion_tables_validate_period_amplitude_and_shape() {
        let header = "command = 'visual_configure'\nid = 'configure-motion'\nmode = 'multipass'\n";
        for period in [32, 300, 600] {
            let text = format!(
                "{header}[motion]\ncase_id = 'motion-sweep'\nperiod_frames = {period}\nyaw_deg = 10.0\npitch_deg = 5.0\nroll_deg = 2.5"
            );
            assert!(matches!(
                read(&text),
                Ok(Request::Visual(visual::Request::Configure {
                    motion: Some(_),
                    pose: None,
                    ..
                }))
            ));
        }
        for period in [0, 1, 2, 31, 601, -1] {
            let text = format!(
                "{header}[motion]\ncase_id = 'motion-sweep'\nperiod_frames = {period}\nyaw_deg = 10.0\npitch_deg = 5.0\nroll_deg = 2.5"
            );
            assert!(
                matches!(
                    read(&text),
                    Err(RequestErrorKind::InvalidMotion(
                        visual::MotionError::PeriodOutOfRange(_)
                    )) | Err(RequestErrorKind::Parse(_))
                ),
                "period {period}"
            );
        }
        for (angle, value) in [
            ("yaw", "45.1"),
            ("pitch", "-0.1"),
            ("roll", "90.0"),
            ("yaw", "nan"),
            ("pitch", "inf"),
        ] {
            let text = format!(
                "{header}[motion]\ncase_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = {}\npitch_deg = {}\nroll_deg = {}",
                if angle == "yaw" { value } else { "0.0" },
                if angle == "pitch" { value } else { "0.0" },
                if angle == "roll" { value } else { "0.0" },
            );
            assert!(
                matches!(
                    read(&text),
                    Err(RequestErrorKind::InvalidMotion(
                        visual::MotionError::AmplitudeOutOfRange(_)
                            | visual::MotionError::NonFiniteAmplitude(_)
                    ))
                ),
                "{angle} {value}"
            );
        }
        for id in ["", "../escape", "a/b", "é", &"x".repeat(65)] {
            let text = format!(
                "{header}[motion]\ncase_id = '{id}'\nperiod_frames = 32\nyaw_deg = 1.0\npitch_deg = 0.0\nroll_deg = 0.0"
            );
            assert!(matches!(
                read(&text),
                Err(RequestErrorKind::InvalidMotion(
                    visual::MotionError::InvalidCaseId(_)
                ))
            ));
        }
        for text in [
            format!(
                "{header}[motion]\ncase_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = 1.0"
            ),
            format!(
                "{header}[motion]\ncase_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = 1.0\npitch_deg = 0.0\nroll_deg = 0.0\nunknown = 1"
            ),
        ] {
            assert!(matches!(read(&text), Err(RequestErrorKind::Parse(_))));
        }
    }

    #[test]
    fn a_motion_table_and_a_pose_table_are_mutually_exclusive() {
        let text = "command = 'visual_configure'\nid = 'both'\nmode = 'multipass'\n\
                    [pose]\ncase_id = 'sweep-1'\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0\n\
                    [motion]\ncase_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = 1.0\npitch_deg = 0.0\nroll_deg = 0.0";
        assert!(matches!(read(text), Err(RequestErrorKind::MotionWithPose)));
    }

    #[test]
    fn motion_tables_are_only_accepted_by_visual_configure() {
        for (command, extra, expected) in [
            ("visual_end", "\nid = 'end-motion'", Command::VisualEnd),
            ("record", "", Command::Record),
            ("screenshot", "\nid = 'shot'", Command::Screenshot),
            ("scenario", "\npath = 'climb.toml'", Command::Scenario),
        ] {
            let text = format!(
                "command = '{command}'{extra}\n[motion]\ncase_id = 'motion-sweep'\nperiod_frames = 32\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0"
            );
            assert!(
                matches!(
                    read(&text),
                    Err(RequestErrorKind::MotionOnCommand(command)) if command == expected
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn motion_capture_is_only_accepted_by_screenshot_and_carries_its_phase() {
        assert!(matches!(
            read("command = 'screenshot'\nid = 'shot'\n[motion_capture]\nphase = 7"),
            Ok(Request::Screenshot {
                motion: Some(7),
                ..
            })
        ));
        assert!(matches!(
            read("command = 'screenshot'\nid = 'shot'"),
            Ok(Request::Screenshot { motion: None, .. })
        ));
        for command in [
            "visual_configure",
            "visual_end",
            "record",
            "stop_recording",
            "stop_replay",
            "stop_scenario",
        ] {
            let extra = match command {
                "visual_configure" => "\nid = 'configure'\nmode = 'multipass'",
                "visual_end" => "\nid = 'end'",
                _ => "",
            };
            let text = format!("command = '{command}'{extra}\n[motion_capture]\nphase = 0");
            assert!(
                matches!(
                    read(&text),
                    Err(RequestErrorKind::MotionCaptureOnCommand(_))
                ),
                "{command}"
            );
        }
        for phase in ["-1", "4294967296", "'x'"] {
            let text =
                format!("command = 'screenshot'\nid = 'shot'\n[motion_capture]\nphase = {phase}");
            assert!(matches!(read(&text), Err(RequestErrorKind::Parse(_))));
        }
        assert!(matches!(
            read("command = 'screenshot'\nid = 'shot'\n[motion_capture]\nphase = 0\nunknown = 1"),
            Err(RequestErrorKind::Parse(_))
        ));
    }
}
