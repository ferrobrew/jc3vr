//! The request file: a TOML file a script drops into the configuration directory to start or stop
//! the test tools without the debug UI or a key. The game thread looks for it once a second, and
//! reads, deletes, and acts on it.
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
    if !path.exists() {
        return;
    }
    match take(&path).and_then(|text| parse(&path, &text)) {
        Ok(request) => act(request),
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
}

/// Why a request file could not be used.
#[derive(Debug)]
struct RequestError {
    path: PathBuf,
    kind: RequestErrorKind,
}

#[derive(Debug)]
enum RequestErrorKind {
    Read(std::io::Error),
    Remove(std::io::Error),
    Parse(toml::de::Error),
    /// The command needs a path and has none.
    NoPath(Command),
    /// The scenario folder is unknown, so a relative path cannot be resolved.
    NoFolder,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "testing: request file {}: ", self.path.display())?;
        match &self.kind {
            RequestErrorKind::Read(e) => write!(f, "could not read it: {e}"),
            RequestErrorKind::Remove(e) => {
                write!(f, "could not remove it, so it is not acted on: {e}")
            }
            RequestErrorKind::Parse(e) => write!(f, "could not parse it: {e}"),
            RequestErrorKind::NoPath(command) => write!(f, "{command:?} needs a path"),
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
    path: Option<String>,
    #[serde(default)]
    start: StartMode,
    #[serde(default)]
    pacing: Pacing,
}

/// Read the file and remove it, so that it is acted on once. A file that cannot be removed is not
/// acted on, or it would be acted on again every second.
fn take(path: &Path) -> Result<String, RequestError> {
    let error = |kind| RequestError {
        path: path.to_owned(),
        kind,
    };
    let text = std::fs::read_to_string(path).map_err(|e| error(RequestErrorKind::Read(e)))?;
    std::fs::remove_file(path).map_err(|e| error(RequestErrorKind::Remove(e)))?;
    Ok(text)
}

fn parse(path: &Path, text: &str) -> Result<Request, RequestError> {
    let error = |kind| RequestError {
        path: path.to_owned(),
        kind,
    };
    let file: RequestFile = toml::from_str(text).map_err(|e| error(RequestErrorKind::Parse(e)))?;
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
    Ok(match file.command {
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
    use pretty_assertions::assert_eq;

    use super::*;

    fn read(text: &str) -> Result<Request, RequestErrorKind> {
        parse(Path::new("C:/config/test_request.toml"), text).map_err(|e| e.kind)
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
}
