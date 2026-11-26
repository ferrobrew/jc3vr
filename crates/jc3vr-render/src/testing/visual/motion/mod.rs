use std::collections::VecDeque;

use jc3vr_view::vr::pose_control::PoseValues;
use serde::{Deserialize, Serialize};

use crate::testing::visual::{
    OrientationOffset, PoseEvidence, PoseTransform, RequestId, TranslationOffset, pose_matches,
};

pub(crate) const MIN_PERIOD_FRAMES: u32 = 32;
pub(crate) const MAX_PERIOD_FRAMES: u32 = 600;
pub(crate) const MAX_AMPLITUDE_DEG: f32 = 45.0;
/// The largest translation amplitude along one camera axis, metres.
pub(crate) const MAX_TRANSLATION_M: f32 = 20.0;
pub const HISTORY_FRAMES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionError {
    InvalidCaseId(&'static str),
    PeriodOutOfRange(u32),
    NonFiniteAmplitude(&'static str),
    AmplitudeOutOfRange(&'static str),
    TranslationOutOfRange(&'static str),
}

impl std::fmt::Display for MotionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCaseId(reason) => write!(f, "visual motion: invalid case id: {reason}"),
            Self::PeriodOutOfRange(period) => write!(
                f,
                "visual motion: period_frames {period} is outside [{MIN_PERIOD_FRAMES}, \
                 {MAX_PERIOD_FRAMES}]"
            ),
            Self::NonFiniteAmplitude(angle) => {
                write!(f, "visual motion: the {angle} amplitude is not finite")
            }
            Self::AmplitudeOutOfRange(angle) => write!(
                f,
                "visual motion: the {angle} amplitude is outside [0, {MAX_AMPLITUDE_DEG}]"
            ),
            Self::TranslationOutOfRange(axis) => write!(
                f,
                "visual motion: the {axis} translation amplitude is not finite or outside \
                 [-{MAX_TRANSLATION_M}, {MAX_TRANSLATION_M}]"
            ),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionTable {
    case_id: String,
    period_frames: u32,
    yaw_deg: f32,
    pitch_deg: f32,
    roll_deg: f32,
    #[serde(default)]
    right_m: f32,
    #[serde(default)]
    up_m: f32,
    #[serde(default)]
    forward_m: f32,
}

/// A periodic camera trajectory about an anchor: the angles swing as `sin θ` (pitch as `sin 2θ`)
/// and the camera-local translation as `sin θ`, with `θ` the phase's fraction of the period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "MotionTable")]
pub struct MotionRequest {
    case_id: RequestId,
    period_frames: u32,
    yaw_deg: f32,
    pitch_deg: f32,
    roll_deg: f32,
    right_m: f32,
    up_m: f32,
    forward_m: f32,
}

impl MotionRequest {
    pub(crate) fn new(
        case_id: RequestId,
        period_frames: u32,
        yaw_deg: f32,
        pitch_deg: f32,
        roll_deg: f32,
    ) -> Result<Self, MotionError> {
        if !(MIN_PERIOD_FRAMES..=MAX_PERIOD_FRAMES).contains(&period_frames) {
            return Err(MotionError::PeriodOutOfRange(period_frames));
        }
        for (angle, value) in [("yaw", yaw_deg), ("pitch", pitch_deg), ("roll", roll_deg)] {
            if !value.is_finite() {
                return Err(MotionError::NonFiniteAmplitude(angle));
            }
            if !(0.0..=MAX_AMPLITUDE_DEG).contains(&value) {
                return Err(MotionError::AmplitudeOutOfRange(angle));
            }
        }
        Ok(Self {
            case_id,
            period_frames,
            yaw_deg,
            pitch_deg,
            roll_deg,
            right_m: 0.0,
            up_m: 0.0,
            forward_m: 0.0,
        })
    }

    /// Add a camera-local translation amplitude, in metres along each axis.
    pub(crate) fn with_translation(
        mut self,
        right_m: f32,
        up_m: f32,
        forward_m: f32,
    ) -> Result<Self, MotionError> {
        for (axis, value) in [("right", right_m), ("up", up_m), ("forward", forward_m)] {
            if !value.is_finite() || value.abs() > MAX_TRANSLATION_M {
                return Err(MotionError::TranslationOutOfRange(axis));
            }
        }
        self.right_m = right_m;
        self.up_m = up_m;
        self.forward_m = forward_m;
        Ok(self)
    }

    pub(crate) fn period_frames(&self) -> u32 {
        self.period_frames
    }

    pub(crate) fn offset(&self, phase: u32) -> OrientationOffset {
        let theta = std::f64::consts::TAU * f64::from(phase) / f64::from(self.period_frames);
        OrientationOffset {
            yaw_deg: (f64::from(self.yaw_deg) * theta.sin()) as f32,
            pitch_deg: (f64::from(self.pitch_deg) * (2.0 * theta).sin()) as f32,
            roll_deg: (f64::from(self.roll_deg) * theta.sin()) as f32,
        }
    }

    pub(crate) fn translation(&self, phase: u32) -> TranslationOffset {
        let swing =
            (std::f64::consts::TAU * f64::from(phase) / f64::from(self.period_frames)).sin();
        TranslationOffset {
            right: (f64::from(self.right_m) * swing) as f32,
            up: (f64::from(self.up_m) * swing) as f32,
            forward: (f64::from(self.forward_m) * swing) as f32,
        }
    }

    pub(crate) fn target(&self, anchor: PoseValues, phase: u32) -> PoseValues {
        PoseValues::from_pose(
            anchor.position + anchor.orientation() * self.translation(phase).vector(),
            anchor.orientation() * self.offset(phase).quaternion(),
        )
    }

    pub(crate) fn evidence(
        &self,
        anchor: PoseValues,
        phase: u32,
        applied: PoseValues,
    ) -> PoseEvidence {
        PoseEvidence {
            case_id: self.case_id.clone(),
            offset: self.offset(phase),
            translation: self.translation(phase),
            anchor: PoseTransform::from(anchor),
            applied: PoseTransform::from(applied),
        }
    }
}

impl TryFrom<MotionTable> for MotionRequest {
    type Error = MotionError;

    fn try_from(table: MotionTable) -> Result<Self, Self::Error> {
        let case_id = RequestId::parse(&table.case_id).map_err(MotionError::InvalidCaseId)?;
        Self::new(
            case_id,
            table.period_frames,
            table.yaw_deg,
            table.pitch_deg,
            table.roll_deg,
        )?
        .with_translation(table.right_m, table.up_m, table.forward_m)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MotionHistoryEntry {
    pub phase: u32,
    pub rendered_frame: i64,
    pub applied: PoseTransform,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MotionMetadata {
    pub request: MotionRequest,
    pub phase: u32,
    pub cycle: u64,
    pub rendered_frame: i64,
    pub history: Vec<MotionHistoryEntry>,
}

#[derive(Debug, Clone)]
pub struct MotionState {
    request: MotionRequest,
    phase: u32,
    cycle: u64,
    history: VecDeque<MotionHistoryEntry>,
    running: bool,
    anchor: Option<PoseValues>,
    last_rendered_frame: Option<i64>,
}

impl MotionState {
    pub(crate) fn new(request: MotionRequest) -> Self {
        Self {
            request,
            phase: 0,
            cycle: 0,
            history: VecDeque::with_capacity(HISTORY_FRAMES),
            running: false,
            anchor: None,
            last_rendered_frame: None,
        }
    }

    pub(crate) fn request(&self) -> &MotionRequest {
        &self.request
    }

    pub(crate) fn phase(&self) -> u32 {
        self.phase
    }

    #[cfg(test)]
    pub(crate) fn cycle(&self) -> u64 {
        self.cycle
    }

    #[cfg(test)]
    pub(crate) fn history(&self) -> &VecDeque<MotionHistoryEntry> {
        &self.history
    }

    pub(crate) fn running(&self) -> bool {
        self.running
    }

    pub(crate) fn anchor(&self) -> Option<PoseValues> {
        self.anchor
    }

    pub(crate) fn begin(&mut self, anchor: PoseValues) {
        self.running = true;
        self.phase = 0;
        self.cycle = 0;
        self.history.clear();
        self.anchor = Some(anchor);
        self.last_rendered_frame = None;
    }

    pub(crate) fn advance(
        &mut self,
        rendered_frame: i64,
        applied: PoseValues,
    ) -> Result<AdvanceOutcome, MotionFrameError> {
        if self.last_rendered_frame == Some(rendered_frame) {
            return Ok(AdvanceOutcome::Duplicate);
        }
        let anchor = self.anchor.ok_or(MotionFrameError::NoAnchor)?;
        let expected = self.request.target(anchor, self.phase);
        if !pose_matches(applied, expected) {
            return Err(MotionFrameError::PoseMismatch {
                phase: self.phase,
                rendered_frame,
            });
        }
        if self.history.len() == HISTORY_FRAMES {
            self.history.pop_front();
        }
        self.history.push_back(MotionHistoryEntry {
            phase: self.phase,
            rendered_frame,
            applied: PoseTransform::from(applied),
        });
        self.last_rendered_frame = Some(rendered_frame);
        self.phase += 1;
        if self.phase == self.request.period_frames {
            self.phase = 0;
            self.cycle += 1;
        }
        Ok(AdvanceOutcome::Advanced(
            self.request.target(anchor, self.phase),
        ))
    }

    pub(crate) fn capture_due(&self, target: u32) -> bool {
        self.running
            && target < self.request.period_frames
            && self.phase == target
            && self.cycle >= 1
            && self.history.len() == HISTORY_FRAMES
            && self.history_is_chronological()
    }

    pub(crate) fn metadata(&self, rendered_frame: i64) -> MotionMetadata {
        MotionMetadata {
            request: self.request.clone(),
            phase: self.phase,
            cycle: self.cycle,
            rendered_frame,
            history: self.history.iter().cloned().collect(),
        }
    }

    fn history_is_chronological(&self) -> bool {
        let mut expected = self.phase;
        for entry in self.history.iter().rev() {
            expected = previous_phase(expected, self.request.period_frames);
            if entry.phase != expected {
                return false;
            }
        }
        self.history
            .iter()
            .zip(self.history.iter().skip(1))
            .all(|(earlier, later)| earlier.rendered_frame < later.rendered_frame)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AdvanceOutcome {
    Advanced(PoseValues),
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MotionFrameError {
    NoAnchor,
    PoseMismatch { phase: u32, rendered_frame: i64 },
}

impl std::fmt::Display for MotionFrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAnchor => write!(f, "visual motion: the scheduler has no anchor"),
            Self::PoseMismatch {
                phase,
                rendered_frame,
            } => write!(
                f,
                "visual motion: frame {rendered_frame} did not render phase {phase}'s pose"
            ),
        }
    }
}

pub(crate) fn previous_phase(phase: u32, period_frames: u32) -> u32 {
    (phase + period_frames - 1) % period_frames
}

pub fn anchor_matches(anchor: PoseValues, base: PoseValues) -> bool {
    pose_matches(base, anchor)
}

#[cfg(test)]
mod tests;
