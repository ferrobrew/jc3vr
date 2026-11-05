//! The evaluated rig against the one the payload ran: each row beside the recorded rig frame
//! (`Rig`, from version 8) nearest before it, and how far the two agree.

use body_rig::{RigOutput, StepPhase, angle};
use glam::Vec2;
use tracking_format::{
    Recording,
    event::{EventKind, RigResult},
};

/// What the payload's rig returned for a row's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RecordedOutput {
    pub body_yaw: f32,
    /// The step phase, `None` for a label this build does not know.
    pub phase: Option<StepPhase>,
    /// The neck's offset from the pelvis, `(x, z)`.
    pub offset: Vec2,
}

impl RecordedOutput {
    pub fn from_result(result: &RigResult) -> Self {
        Self {
            body_yaw: result.body_yaw,
            phase: phase_of(&result.phase),
            offset: result.offset,
        }
    }

    /// The evaluated body yaw less the recorded one, radians, wrapped.
    pub fn yaw_difference(&self, output: &RigOutput) -> f32 {
        angle::wrap(output.body_yaw - self.body_yaw)
    }

    /// Whether the evaluated phase is the recorded one, unless the recorded one is unknown.
    pub fn phase_matches(&self, output: &RigOutput) -> Option<bool> {
        self.phase.map(|phase| phase == output.phase)
    }

    /// The evaluated neck offset's length less the recorded one's, metres.
    pub fn offset_difference(&self, output: &RigOutput) -> f32 {
        output.solve.offset.length() - self.offset.length()
    }
}

/// How far the evaluated rig agrees with the recorded one over the rows that have a recorded
/// frame.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Comparison {
    /// Rows with a recorded frame.
    pub rows: u64,
    /// The mean and the largest absolute difference of the body yaw, degrees.
    pub mean_yaw_diff_deg: f64,
    pub max_yaw_diff_deg: f32,
    /// The share of rows with a known recorded phase whose phase agrees.
    pub phase_agreement: f64,
    /// The mean absolute difference of the neck offset's length, metres.
    pub mean_offset_diff_m: f64,
}

/// The recorded rig frames by time, for pairing with rows on the tracking frames' clock.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct RecordedTrack {
    samples: Vec<(u64, RecordedOutput)>,
}

impl RecordedTrack {
    pub(crate) fn new(recording: &Recording) -> Self {
        let mut samples: Vec<(u64, RecordedOutput)> = recording
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::Rig(sample) => {
                    Some((event.time_ns, RecordedOutput::from_result(&sample.output)))
                }
                _ => None,
            })
            .collect();
        samples.sort_by_key(|(at, _)| *at);
        Self { samples }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The latest recorded frame at or before `time_ns`, within [`PAIR_NS`].
    pub(crate) fn at(&self, time_ns: u64) -> Option<RecordedOutput> {
        let index = self.samples.partition_point(|(at, _)| *at <= time_ns);
        let (at, output) = *self.samples.get(index.checked_sub(1)?)?;
        (time_ns - at <= PAIR_NS).then_some(output)
    }
}

/// The running sums behind a [`Comparison`].
#[derive(Debug, Clone, Default)]
pub(crate) struct ComparisonAccumulator {
    rows: u64,
    yaw_sum_deg: f64,
    max_yaw_deg: f32,
    phases: u64,
    phases_agreeing: u64,
    offset_sum_m: f64,
}

impl ComparisonAccumulator {
    pub(crate) fn observe(&mut self, output: &RigOutput, recorded: &RecordedOutput) {
        let yaw = recorded.yaw_difference(output).abs().to_degrees();
        self.rows += 1;
        self.yaw_sum_deg += f64::from(yaw);
        self.max_yaw_deg = self.max_yaw_deg.max(yaw);
        if let Some(agrees) = recorded.phase_matches(output) {
            self.phases += 1;
            self.phases_agreeing += u64::from(agrees);
        }
        self.offset_sum_m += f64::from(recorded.offset_difference(output).abs());
    }

    /// The comparison, if any row had a recorded frame.
    pub(crate) fn finish(self) -> Option<Comparison> {
        (self.rows > 0).then(|| {
            let rows = self.rows as f64;
            Comparison {
                rows: self.rows,
                mean_yaw_diff_deg: self.yaw_sum_deg / rows,
                max_yaw_diff_deg: self.max_yaw_deg,
                phase_agreement: if self.phases > 0 {
                    self.phases_agreeing as f64 / self.phases as f64
                } else {
                    0.0
                },
                mean_offset_diff_m: self.offset_sum_m / rows,
            }
        })
    }
}

/// A row on the tracking frames' clock takes a recorded rig frame at most this old, nanoseconds:
/// a few rendered frames.
const PAIR_NS: u64 = 50_000_000;

const PHASES: [StepPhase; 4] = [
    StepPhase::Idle,
    StepPhase::Stepping,
    StepPhase::Settling,
    StepPhase::Drifting,
];

fn phase_of(label: &str) -> Option<StepPhase> {
    PHASES.into_iter().find(|phase| phase.label() == label)
}
