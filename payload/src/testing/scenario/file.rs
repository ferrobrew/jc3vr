//! The scenario file: TOML naming a recording, how to replay it, and what to expect while it
//! replays, read and checked into a [`Scenario`].

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Deserialize;
use tracing::Level;

use crate::{
    controllers::menu::status::Health,
    testing::{
        replay::{Pacing, ReplayOptions, StartMode},
        scenario::expect::{Check, Condition},
        signals::{self, Signal, SignalKind},
    },
};

/// A scenario, checked.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Scenario {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    /// The scenario file.
    pub(crate) path: PathBuf,
    /// The recording, resolved against the scenario file's folder.
    pub(crate) recording: PathBuf,
    pub(crate) options: ReplayOptions,
    /// How long the expectations are still checked after the replay ends.
    pub(crate) settle: Duration,
    pub(crate) expectations: Vec<Expectation>,
}

/// One expectation: what to read, and what it must do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Expectation {
    pub(crate) name: String,
    pub(crate) probe: Probe,
    pub(crate) check: Check,
    pub(crate) condition: Condition,
}

/// What an expectation reads each frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Probe {
    /// A published signal. With `delta`, a counter reads how far it grew since the replay started.
    Signal { signal: Signal, delta: bool },
    /// Whether a `VR: STATUS` row shows the health and the detail asked for.
    Status {
        label: &'static str,
        health: Option<Health>,
        /// A text the row's detail must contain.
        detail: Option<String>,
    },
    /// How many log lines at `level` or above contained `pattern` since the replay started.
    Log {
        pattern: String,
        level: Option<Level>,
    },
}

/// Why a scenario file could not be used.
#[derive(Debug)]
pub(crate) struct ScenarioError {
    pub(crate) path: PathBuf,
    pub(crate) kind: ScenarioErrorKind,
}

#[derive(Debug)]
pub(crate) enum ScenarioErrorKind {
    Read(std::io::Error),
    Parse(toml::de::Error),
    NoExpectations,
    NoRecording,
    Expectation {
        index: usize,
        name: Option<String>,
        problem: Problem,
    },
}

/// What is wrong with one expectation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Problem {
    /// None of `signal`, `status`, and `log` is given.
    NoProbe,
    /// More than one of them is.
    SeveralProbes,
    UnknownSignal(String),
    UnknownStatusRow(String),
    UnknownHealth(String),
    UnknownLevel(String),
    /// More than one of the conditions is given.
    SeveralConditions,
    /// A measurement with no condition to hold it to.
    NeedsCondition,
    /// A condition of the wrong kind for the probe: a number for a flag, or a flag for a number.
    ConditionDoesNotFit(&'static str),
    /// `delta` on anything but a counter.
    DeltaNeedsCounter,
    /// `health` or `detail` without `status`.
    StatusFieldsNeedStatus,
    /// `level` without `log`.
    LevelNeedsLog,
    /// A log probe with neither a pattern nor a level, which would match every line.
    EmptyLog,
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scenario {}: ", self.path.display())?;
        match &self.kind {
            ScenarioErrorKind::Read(e) => write!(f, "could not read the file: {e}"),
            ScenarioErrorKind::Parse(e) => write!(f, "could not parse the file: {e}"),
            ScenarioErrorKind::NoExpectations => write!(f, "it has no [[expect]] entries"),
            ScenarioErrorKind::NoRecording => write!(f, "its recording is empty"),
            ScenarioErrorKind::Expectation {
                index,
                name,
                problem,
            } => {
                write!(f, "expectation {}", index + 1)?;
                if let Some(name) = name {
                    write!(f, " ({name})")?;
                }
                write!(f, ": {problem}")
            }
        }
    }
}

impl std::error::Error for ScenarioError {}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Problem::NoProbe => write!(f, "it names none of signal, status, and log"),
            Problem::SeveralProbes => {
                write!(f, "it names more than one of signal, status, and log")
            }
            Problem::UnknownSignal(name) => write!(f, "there is no signal {name:?}"),
            Problem::UnknownStatusRow(label) => {
                write!(f, "the VR: STATUS page has no row {label:?}")
            }
            Problem::UnknownHealth(word) => {
                write!(f, "{word:?} is not one of ok, off, waiting, and failed")
            }
            Problem::UnknownLevel(word) => {
                write!(f, "{word:?} is not one of info, warn, and error")
            }
            Problem::SeveralConditions => write!(f, "it gives more than one condition"),
            Problem::NeedsCondition => write!(
                f,
                "a measurement needs a condition (below, above, at_least, at_most, or equals)"
            ),
            Problem::ConditionDoesNotFit(what) => write!(f, "{what}"),
            Problem::DeltaNeedsCounter => write!(f, "delta applies to counters only"),
            Problem::StatusFieldsNeedStatus => write!(f, "health and detail apply to status only"),
            Problem::LevelNeedsLog => write!(f, "level applies to log only"),
            Problem::EmptyLog => write!(f, "a log probe needs a pattern or a level"),
        }
    }
}

/// Read and check the scenario file at `path`.
pub(crate) fn read(path: &Path) -> Result<Scenario, ScenarioError> {
    let error = |kind| ScenarioError {
        path: path.to_owned(),
        kind,
    };
    let text = std::fs::read_to_string(path).map_err(|e| error(ScenarioErrorKind::Read(e)))?;
    parse(path, &text)
}

/// Check the scenario `text`, read from `path`.
pub(crate) fn parse(path: &Path, text: &str) -> Result<Scenario, ScenarioError> {
    let error = |kind| ScenarioError {
        path: path.to_owned(),
        kind,
    };
    let file: ScenarioFile =
        toml::from_str(text).map_err(|e| error(ScenarioErrorKind::Parse(e)))?;
    if file.recording.trim().is_empty() {
        return Err(error(ScenarioErrorKind::NoRecording));
    }
    if file.expect.is_empty() {
        return Err(error(ScenarioErrorKind::NoExpectations));
    }
    let expectations = file
        .expect
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let name = entry.name.clone();
            entry.check().map_err(|problem| {
                error(ScenarioErrorKind::Expectation {
                    index,
                    name,
                    problem,
                })
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let recording = PathBuf::from(file.recording.trim());
    let recording = if recording.is_absolute() {
        recording
    } else {
        path.parent().unwrap_or(Path::new("")).join(recording)
    };
    let settle = if file.settle_s.is_finite() {
        file.settle_s.clamp(0.0, MAX_SETTLE_S)
    } else {
        DEFAULT_SETTLE_S
    };
    Ok(Scenario {
        name: file.name.unwrap_or_else(|| {
            path.file_stem().map_or_else(
                || "scenario".to_owned(),
                |s| s.to_string_lossy().into_owned(),
            )
        }),
        description: file.description,
        path: path.to_owned(),
        recording,
        options: ReplayOptions {
            start: file.start,
            pacing: file.pacing,
        },
        settle: Duration::from_secs_f32(settle),
        expectations,
    })
}

const DEFAULT_SETTLE_S: f32 = 2.0;
const MAX_SETTLE_S: f32 = 600.0;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioFile {
    name: Option<String>,
    description: Option<String>,
    recording: String,
    #[serde(default)]
    start: StartMode,
    #[serde(default)]
    pacing: Pacing,
    #[serde(default = "default_settle")]
    settle_s: f32,
    #[serde(default)]
    expect: Vec<ExpectationFile>,
}

fn default_settle() -> f32 {
    DEFAULT_SETTLE_S
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectationFile {
    name: Option<String>,
    signal: Option<String>,
    status: Option<String>,
    log: Option<String>,
    health: Option<String>,
    detail: Option<String>,
    level: Option<String>,
    check: Option<Check>,
    #[serde(default)]
    delta: Option<bool>,
    is: Option<bool>,
    below: Option<f64>,
    above: Option<f64>,
    at_least: Option<f64>,
    at_most: Option<f64>,
    equals: Option<f64>,
}

/// What a probe reads, for fitting its condition and defaults.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Reads {
    Flag,
    Counter,
    Measure,
    LogCount,
}

impl ExpectationFile {
    fn check(self) -> Result<Expectation, Problem> {
        let probes = [
            self.signal.is_some(),
            self.status.is_some(),
            self.log.is_some(),
        ];
        match probes.iter().filter(|&&p| p).count() {
            0 => return Err(Problem::NoProbe),
            1 => {}
            _ => return Err(Problem::SeveralProbes),
        }
        if self.status.is_none() && (self.health.is_some() || self.detail.is_some()) {
            return Err(Problem::StatusFieldsNeedStatus);
        }
        if self.log.is_none() && self.level.is_some() {
            return Err(Problem::LevelNeedsLog);
        }
        let (probe, reads, label) = self.probe()?;
        let condition = self.condition(reads)?;
        let check = self.check.unwrap_or(match reads {
            Reads::Flag => Check::Eventually,
            Reads::Measure => Check::Always,
            Reads::Counter | Reads::LogCount => Check::AtEnd,
        });
        let default_name = || match probe {
            // A row's default name says what it must show; its condition is always `is true`.
            Probe::Status { .. } => label.clone(),
            _ => format!("{label} {}", condition.describe()),
        };
        Ok(Expectation {
            name: self.name.clone().unwrap_or_else(default_name),
            probe,
            check,
            condition,
        })
    }

    /// The probe, what it reads, and a label for a default name.
    fn probe(&self) -> Result<(Probe, Reads, String), Problem> {
        if let Some(name) = &self.signal {
            let signal = Signal::parse(name).ok_or_else(|| Problem::UnknownSignal(name.clone()))?;
            let reads = match signal.kind() {
                SignalKind::Flag => Reads::Flag,
                SignalKind::Counter => Reads::Counter,
                SignalKind::Measure => Reads::Measure,
            };
            // A counter counts since the payload loaded, so by default it is read from the
            // replay's start.
            let delta = self.delta.unwrap_or(reads == Reads::Counter);
            if delta && reads != Reads::Counter {
                return Err(Problem::DeltaNeedsCounter);
            }
            let label = if delta {
                format!("{} growth", signal.name())
            } else {
                signal.name().to_owned()
            };
            return Ok((Probe::Signal { signal, delta }, reads, label));
        }
        if self.delta.is_some() {
            return Err(Problem::DeltaNeedsCounter);
        }
        if let Some(row) = &self.status {
            let label = signals::status_row_label(row)
                .ok_or_else(|| Problem::UnknownStatusRow(row.clone()))?;
            let health = self
                .health
                .as_deref()
                .map(|word| {
                    signals::parse_health(word).ok_or_else(|| Problem::UnknownHealth(word.into()))
                })
                .transpose()?;
            let shows = match (&health, &self.detail) {
                (Some(h), Some(d)) => format!("shows {} with {d:?}", h.word()),
                (Some(h), None) => format!("shows {}", h.word()),
                (None, Some(d)) => format!("shows {d:?}"),
                (None, None) => "shows OK".to_owned(),
            };
            let probe = Probe::Status {
                label,
                // A row with nothing asked of it must read OK.
                health: health.or(if self.detail.is_none() {
                    Some(Health::Ok)
                } else {
                    None
                }),
                detail: self.detail.clone(),
            };
            return Ok((probe, Reads::Flag, format!("STATUS {label} {shows}")));
        }
        let pattern = self.log.clone().unwrap_or_default();
        let level = self
            .level
            .as_deref()
            .map(|word| parse_level(word).ok_or_else(|| Problem::UnknownLevel(word.into())))
            .transpose()?;
        if pattern.is_empty() && level.is_none() {
            return Err(Problem::EmptyLog);
        }
        let label = match level {
            Some(level) => format!("{level} log lines matching {pattern:?}"),
            None => format!("log lines matching {pattern:?}"),
        };
        Ok((Probe::Log { pattern, level }, Reads::LogCount, label))
    }

    fn condition(&self, reads: Reads) -> Result<Condition, Problem> {
        let numeric = [
            self.below.map(Condition::Below),
            self.above.map(Condition::Above),
            self.at_least.map(Condition::AtLeast),
            self.at_most.map(Condition::AtMost),
            self.equals.map(Condition::Equals),
        ];
        let numeric: Vec<Condition> = numeric.into_iter().flatten().collect();
        let given = numeric.len() + usize::from(self.is.is_some());
        if given > 1 {
            return Err(Problem::SeveralConditions);
        }
        match reads {
            Reads::Flag => {
                if !numeric.is_empty() {
                    return Err(Problem::ConditionDoesNotFit(
                        "a flag takes `is`, not a number",
                    ));
                }
                Ok(Condition::Is(self.is.unwrap_or(true)))
            }
            Reads::Counter | Reads::Measure | Reads::LogCount => {
                if self.is.is_some() {
                    return Err(Problem::ConditionDoesNotFit(
                        "a number takes below, above, at_least, at_most, or equals, not `is`",
                    ));
                }
                match numeric.first() {
                    Some(condition) => Ok(*condition),
                    None if reads == Reads::Measure => Err(Problem::NeedsCondition),
                    None => Ok(Condition::AtLeast(1.0)),
                }
            }
        }
    }
}

/// The minimum level a scenario file names.
fn parse_level(word: &str) -> Option<Level> {
    match word.to_ascii_lowercase().as_str() {
        "info" => Some(Level::INFO),
        "warn" | "warning" => Some(Level::WARN),
        "error" => Some(Level::ERROR),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
