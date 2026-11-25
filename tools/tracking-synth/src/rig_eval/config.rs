//! The rig's calibration and tuning over a recording: the command line's, or the ones the
//! recording notes (`RigConfig`, from version 8) where the command line leaves them out.

use std::fmt;

use body_rig::{PlayerBody, RigTuning};
use tracking_format::{
    Recording,
    event::{EventKind, RigConfig},
};

use crate::rig_eval::EvalOptions;

/// A calibration and tuning a recording noted.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ConfigNote {
    /// Nanoseconds on the recording's clock.
    pub time_ns: u64,
    /// The player's standing height and eye height, metres.
    pub height_m: f32,
    pub eye_height_m: f32,
    /// The rig's body as the payload ran it.
    pub body: PlayerBody,
    pub tuning: RigTuning,
}

/// A noted calibration and tuning the evaluator could not take up.
#[derive(Debug)]
pub struct ConfigError {
    /// Nanoseconds on the recording's clock.
    pub time_ns: u64,
    pub kind: ConfigErrorKind,
}

#[derive(Debug)]
pub enum ConfigErrorKind {
    /// The tuning's JSON does not parse as the rig's tuning.
    Tuning(serde_json::Error),
    /// The payload could not serialize its tuning, and noted why instead.
    Unserialized(String),
    /// A height that is not a positive length.
    Body,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = self.time_ns as f64 / 1.0e9;
        write!(f, "rig-eval: the rig config noted at {at:.3} s ")?;
        match &self.kind {
            ConfigErrorKind::Tuning(error) => {
                write!(f, "has a tuning that does not parse: {error}")
            }
            ConfigErrorKind::Unserialized(reason) => {
                write!(
                    f,
                    "has no tuning, as the payload could not write it: {reason}"
                )
            }
            ConfigErrorKind::Body => write!(f, "has a height that is not a positive length"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ConfigErrorKind::Tuning(error) => Some(error),
            ConfigErrorKind::Unserialized(_) | ConfigErrorKind::Body => None,
        }
    }
}

/// What of a recording's calibration and tuning notes an evaluation took up.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RecordedConfig {
    /// How many notes the recording holds that read.
    pub notes: usize,
    /// The first of them.
    pub first: ConfigNote,
    /// Whether the evaluation ran the noted body rather than the command line's.
    pub body: bool,
    /// Whether the evaluation ran the noted tuning rather than the command line's.
    pub tuning: bool,
}

/// Every calibration and tuning `recording` notes, in time order, and the notes that could not be
/// read.
pub(crate) fn config_notes(recording: &Recording) -> (Vec<ConfigNote>, Vec<ConfigError>) {
    let mut notes = Vec::new();
    let mut errors = Vec::new();
    for event in &recording.events {
        let EventKind::RigConfig(config) = &event.kind else {
            continue;
        };
        match read_note(config) {
            Ok((height_m, eye_height_m, body, tuning)) => notes.push(ConfigNote {
                time_ns: event.time_ns,
                height_m,
                eye_height_m,
                body,
                tuning,
            }),
            Err(kind) => errors.push(ConfigError {
                time_ns: event.time_ns,
                kind,
            }),
        }
    }
    (notes, errors)
}

/// The calibration and tuning in effect at each time of an evaluation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConfigResolver {
    base: (PlayerBody, RigTuning),
    notes: Vec<ConfigNote>,
    recorded_body: bool,
    recorded_tuning: bool,
}

impl ConfigResolver {
    /// Always `options`' calibration and tuning.
    pub(crate) fn fixed(options: &EvalOptions) -> Self {
        Self {
            base: (options.body, options.tuning),
            notes: Vec::new(),
            recorded_body: false,
            recorded_tuning: false,
        }
    }

    /// The `notes`' body and tuning where asked for, `options`' otherwise.
    pub(crate) fn new(
        options: &EvalOptions,
        notes: Vec<ConfigNote>,
        recorded_body: bool,
        recorded_tuning: bool,
    ) -> Self {
        Self {
            base: (options.body, options.tuning),
            notes,
            recorded_body,
            recorded_tuning,
        }
    }

    /// The body and tuning at `time_ns`: the latest note's at or before it, or the first note's
    /// before any, since a note repeats what was already in use.
    pub(crate) fn at(&self, time_ns: u64) -> (PlayerBody, RigTuning) {
        let index = self.notes.partition_point(|note| note.time_ns <= time_ns);
        let Some(note) = self.notes.get(index.saturating_sub(1)) else {
            return self.base;
        };
        (
            if self.recorded_body {
                note.body
            } else {
                self.base.0
            },
            if self.recorded_tuning {
                note.tuning
            } else {
                self.base.1
            },
        )
    }

    /// What of the recording's notes the evaluation takes up.
    pub(crate) fn usage(&self) -> Option<RecordedConfig> {
        let first = *self.notes.first()?;
        Some(RecordedConfig {
            notes: self.notes.len(),
            first,
            body: self.recorded_body,
            tuning: self.recorded_tuning,
        })
    }
}

fn read_note(config: &RigConfig) -> Result<(f32, f32, PlayerBody, RigTuning), ConfigErrorKind> {
    let lengths = [
        config.height_m,
        config.eye_height_m,
        config.body_eye_height_m,
        config.body_hip_height_m,
    ];
    if !lengths
        .iter()
        .all(|length| length.is_finite() && *length > 0.0)
    {
        return Err(ConfigErrorKind::Body);
    }
    let value: serde_json::Value =
        serde_json::from_str(&config.tuning_json).map_err(ConfigErrorKind::Tuning)?;
    // The payload notes `{"error": ...}` when it cannot serialize the tuning, which the tuning's
    // defaulted fields would otherwise read as the default tuning.
    if let Some(reason) = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("error"))
    {
        return Err(ConfigErrorKind::Unserialized(
            reason
                .as_str()
                .map_or_else(|| reason.to_string(), str::to_owned),
        ));
    }
    let tuning: RigTuning = serde_json::from_value(value).map_err(ConfigErrorKind::Tuning)?;
    Ok((
        config.height_m,
        config.eye_height_m,
        PlayerBody {
            eye_height: config.body_eye_height_m,
            hip_height: config.body_hip_height_m,
        },
        tuning,
    ))
}
