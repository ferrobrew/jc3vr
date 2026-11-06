//! The offline evaluator of the grounded body rig: the payload's own solver (`body-rig`) run over
//! a recording, so the lean radius, the thresholds, and the neck offset can be tuned against what a
//! player really did before a headset session. See `docs/mod/body/rig.md`.
//!
//! The rig's inputs come from one of three sources ([`PelvisSource`]):
//!
//! - the tracking frames with a simulated pelvis that follows the step drive (`frames`), the only
//!   source a recording before version 8 has;
//! - the rig inputs the payload recorded (`replay`), so a changed rig or tuning runs on exactly the
//!   frames the game's ran;
//! - the tracking frames with the pelvis under the hips of the game's pre-IK pose (`pre_ik`), so
//!   the rig runs against the game's own animation.
//!
//! On the tracking frames, the rig works in the room frame, and the recording's poses are in the
//! cockpit frame, which is the room frame up to the floor's height: the eyes' height over the floor
//! is the STAGE space's when the runtime located it, and the cockpit height plus the calibrated eye
//! height otherwise. The stick moves the room and the body together, so it changes nothing here but
//! the yaw's deadzone.
//!
//! The simulated pelvis is the game's character under the drive: its velocity moves toward the
//! drive's at a fixed acceleration, as the stick locomotion's integrator and the proxy do. The rig
//! runs only while the body is grounded (the procedural body's hand-over case is `on foot`, or the
//! on-foot action set in a recording without body states); elsewhere the pelvis is put back under
//! the neck when the rig resumes.
//!
//! A recenter turns the room with the cockpit frame. A manual or seat recenter then snaps the
//! pelvis under the neck, as the rig's recenter does; a follow bank only turns the frame.
//!
//! A recording from version 8 also notes the payload's rig frames, which each row is compared with
//! (`compare`), and the calibration and tuning in use, which the evaluation runs where the command
//! line leaves them out (`config`).

mod clock;
mod compare;
mod config;
mod csv;
mod frames;
mod pre_ik;
mod replay;
mod summary;

use std::fmt;

use body_rig::{PlayerBody, RigOutput, RigTuning};
use glam::Vec2;
use tracking_format::{Recording, event::EventKind};

pub use crate::rig_eval::{
    clock::VideoClock,
    compare::{Comparison, RecordedOutput},
    config::{ConfigError, ConfigErrorKind, ConfigNote, RecordedConfig},
    csv::{write_compared_csv, write_csv},
};
use crate::rig_eval::{
    compare::RecordedTrack,
    config::{ConfigResolver, config_notes},
    frames::{FrameRun, PelvisDrive, run_frames},
    pre_ik::pre_ik_track,
    replay::run_recorded,
};

/// What the evaluation runs with.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EvalOptions {
    pub body: PlayerBody,
    pub tuning: RigTuning,
    /// How fast the simulated character's velocity reaches the drive's, metres per second squared.
    pub accel_mps2: f32,
}

impl EvalOptions {
    /// The defaults for a player `height` tall with the eyes at `eye_height`.
    pub fn new(height: f32, eye_height: f32) -> Self {
        Self {
            body: PlayerBody::from_heights(height, eye_height),
            tuning: RigTuning::new(),
            accel_mps2: DEFAULT_ACCEL_MPS2,
        }
    }
}

/// The simulated character's acceleration toward the drive, metres per second squared: about the
/// stick locomotion's fixed rate.
pub const DEFAULT_ACCEL_MPS2: f32 = 10.0;

/// One evaluated frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EvalRow {
    /// Seconds on the recording's clock.
    pub time_s: f64,
    /// Seconds into the take's video, when the sidecar maps the frame.
    pub video_s: Option<f64>,
    pub output: RigOutput,
    pub head_yaw: f32,
    /// The head's pitch from level, positive looking up.
    pub head_pitch: f32,
    pub pelvis: Vec2,
    pub speed_mps: f32,
    /// Whether the off hand's stick moves the body.
    pub moving: bool,
}

/// A summary of the evaluation.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct EvalSummary {
    /// Seconds the rig ran.
    pub grounded_s: f64,
    pub steps: u64,
    /// Seconds spent stepping or settling.
    pub driving_s: f64,
    /// Body turns started.
    pub turns: u64,
    /// The median size of a body turn, from where it started to where it settled, degrees.
    pub median_turn_deg: f32,
    /// The size nine in ten body turns are below, degrees.
    pub p90_turn_deg: f32,
    /// The mean absolute angle between the head and the body, degrees.
    pub mean_yaw_error_deg: f64,
    /// The mean absolute angle between the head and the chest, degrees.
    pub mean_chest_error_deg: f64,
    /// The largest neck offset over the pelvis, metres.
    pub max_offset_m: f32,
    /// The mean neck offset over the pelvis, metres.
    pub mean_offset_m: f64,
    /// Seconds in stretches longer than [`HELD_LEAN_S`] in which the body idled with the neck more
    /// than [`HELD_LEAN_M`] out over the pelvis and no crouch: offsets drawn as a bend that never
    /// settled.
    pub held_lean_s: f64,
    pub recenters: u64,
}

/// A held lean is an idle offset longer than this, seconds...
pub const HELD_LEAN_S: f64 = 8.0;

/// ...further out than this, metres.
pub const HELD_LEAN_M: f32 = 0.08;

/// Where the rig's inputs, and the pelvis above all, come from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PelvisSource {
    /// The tracking frames, with a pelvis simulated under the step drive.
    Simulated,
    /// The rig inputs the payload recorded, pelvis and all.
    Recorded,
    /// The tracking frames, with the pelvis under the hips of the game's pre-IK pose.
    PreIk,
}

impl PelvisSource {
    pub const ALL: [PelvisSource; 3] = [
        PelvisSource::Simulated,
        PelvisSource::Recorded,
        PelvisSource::PreIk,
    ];

    /// The source's name on the command line.
    pub fn name(self) -> &'static str {
        match self {
            PelvisSource::Simulated => "simulated",
            PelvisSource::Recorded => "recorded",
            PelvisSource::PreIk => "pre-ik",
        }
    }

    /// The source named `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|source| source.name() == name)
    }

    /// The source a recording is evaluated from when none is asked for: its recorded rig inputs
    /// where it has them, the simulated pelvis otherwise.
    pub fn default_for(recording: &Recording) -> Self {
        if has_rig_samples(recording) {
            PelvisSource::Recorded
        } else {
            PelvisSource::Simulated
        }
    }
}

/// What an evaluation from a chosen source runs with.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReplayOptions {
    /// The calibration, tuning, and simulation the evaluation runs with where the recording notes
    /// none or the flags below leave them.
    pub eval: EvalOptions,
    /// The source, or the recording's [default](PelvisSource::default_for).
    pub source: Option<PelvisSource>,
    /// Whether the recording's noted body replaces `eval`'s.
    pub recorded_body: bool,
    /// Whether the recording's noted tuning replaces `eval`'s.
    pub recorded_tuning: bool,
}

/// One evaluated frame beside the payload's rig frame for it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ComparedRow {
    pub row: EvalRow,
    /// The recorded rig frame's output, when the recording has one for the row.
    pub recorded: Option<RecordedOutput>,
}

/// An evaluation from a chosen source.
#[derive(Debug)]
pub struct Evaluation {
    /// The source that ran.
    pub source: PelvisSource,
    pub rows: Vec<ComparedRow>,
    pub summary: EvalSummary,
    /// How far the evaluated rig agrees with the recorded one, when the recording has rig frames.
    pub comparison: Option<Comparison>,
    /// The recording's calibration and tuning notes, and what of them the evaluation took up.
    pub config: Option<RecordedConfig>,
    /// The notes that could not be read, which the evaluation left out.
    pub config_errors: Vec<ConfigError>,
    /// Pre-IK poses no view placed, for the pre-IK source.
    pub unplaced_poses: usize,
}

/// Why an evaluation could not run from the source asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RigEvalError {
    /// The recorded source over a recording with no rig frames.
    NoRigSamples { version: u16 },
    /// The pre-IK source over a recording with no pre-IK pose a view places.
    NoPreIkPoses { version: u16, unplaced: usize },
}

impl fmt::Display for RigEvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RigEvalError::NoRigSamples { version } => write!(
                f,
                "rig-eval: `--pelvis recorded` needs the rig frames a version 8 recording notes, \
                 and this version {version} recording has none"
            ),
            RigEvalError::NoPreIkPoses { version, unplaced } => write!(
                f,
                "rig-eval: `--pelvis pre-ik` needs the pre-IK poses and views a version 8 recording \
                 notes, and this version {version} recording has none that place the hips \
                 ({unplaced} poses without a view)"
            ),
        }
    }
}

impl std::error::Error for RigEvalError {}

/// Run the rig over `recording` with the simulated pelvis and `options`' calibration and tuning,
/// whatever the recording notes.
pub fn evaluate(
    recording: &Recording,
    options: &EvalOptions,
    clock: Option<&VideoClock>,
) -> (Vec<EvalRow>, EvalSummary) {
    let configs = ConfigResolver::fixed(options);
    let recorded = RecordedTrack::default();
    let (rows, summary, _) = run_frames(
        recording,
        &FrameRun {
            options,
            configs: &configs,
            drive: PelvisDrive::Simulated,
            recorded: &recorded,
            clock,
        },
    );
    (rows.into_iter().map(|row| row.row).collect(), summary)
}

/// Run the rig over `recording` from the source `options` asks for, comparing each row with the
/// recorded rig frame where the recording has one.
pub fn evaluate_replay(
    recording: &Recording,
    options: &ReplayOptions,
    clock: Option<&VideoClock>,
) -> Result<Evaluation, RigEvalError> {
    let source = options
        .source
        .unwrap_or_else(|| PelvisSource::default_for(recording));
    let (notes, config_errors) = config_notes(recording);
    let configs = ConfigResolver::new(
        &options.eval,
        notes,
        options.recorded_body,
        options.recorded_tuning,
    );
    let recorded = RecordedTrack::new(recording);
    let mut unplaced_poses = 0;
    let (rows, summary, comparison) = match source {
        PelvisSource::Recorded => {
            if recorded.is_empty() {
                return Err(RigEvalError::NoRigSamples {
                    version: recording.version,
                });
            }
            run_recorded(recording, &configs, clock)
        }
        PelvisSource::Simulated | PelvisSource::PreIk => {
            let track = (source == PelvisSource::PreIk).then(|| pre_ik_track(recording));
            let drive = match track.as_ref() {
                Some(track) => {
                    unplaced_poses = track.unplaced;
                    if track.points.is_empty() {
                        return Err(RigEvalError::NoPreIkPoses {
                            version: recording.version,
                            unplaced: track.unplaced,
                        });
                    }
                    PelvisDrive::PreIk(track)
                }
                None => PelvisDrive::Simulated,
            };
            run_frames(
                recording,
                &FrameRun {
                    options: &options.eval,
                    configs: &configs,
                    drive,
                    recorded: &recorded,
                    clock,
                },
            )
        }
    };
    Ok(Evaluation {
        source,
        rows,
        summary,
        comparison: comparison.finish(),
        config: configs.usage(),
        config_errors,
        unplaced_poses,
    })
}

fn has_rig_samples(recording: &Recording) -> bool {
    recording
        .events
        .iter()
        .any(|event| matches!(event.kind, EventKind::Rig(_)))
}

#[cfg(test)]
mod tests;
