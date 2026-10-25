//! Shadow-only constrained body fit over G01 observations.
//!
//! One simulation commit owns stance, pelvis, torso, and inferred foot contacts. Tracking validity,
//! geometric residual, and ambiguity remain separate. No result here authorizes an engine write.
//! The bounded search and contact transitions are described in `g03-shadow-fit.md`.

mod anatomy;
mod contacts;
mod equivalence;
mod fit;
mod state;
mod temporal;

#[cfg(test)]
mod tests;

use std::{
    error::Error,
    fmt,
    time::{Duration, Instant},
};

use glam::{Quat, Vec3};

pub(crate) use crate::controllers::body::rig::estimate::{
    anatomy::Anatomy,
    contacts::{ContactState, ReleaseReason, Support},
    state::Estimator,
};
use crate::controllers::{
    body::rig::{
        estimate::contacts::Contact,
        legs::swing::FootPose,
        observation::{Observation, ObservationError, SampleStamp, StepStamp},
    },
    frame::TrackedPose,
};

/// Explicit discontinuities. A recenter preserves world contacts; transport discards them.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Transition {
    Continue,
    Recenter,
    Reseed,
    Teleport,
}

/// One simulation correction, never a render update.
pub(crate) struct Update<'a> {
    pub observation: &'a Observation,
    pub now: Instant,
    pub max_age: Duration,
    pub dt: f32,
    pub anatomy: &'a Anatomy,
    pub support: Option<Support>,
    pub transition: Transition,
}

/// All body landmarks and contacts are inferred, even when the sensor fit is exact.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Confidence {
    Low,
}

/// Tracking flags are copied from the runtime; no solver result changes them.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct TrackingQuality {
    pub position_valid: bool,
    pub orientation_valid: bool,
    pub position_measured: bool,
    pub orientation_measured: bool,
}

impl From<TrackedPose> for TrackingQuality {
    fn from(pose: TrackedPose) -> Self {
        Self {
            position_valid: pose.position_valid,
            orientation_valid: pose.orientation_valid,
            position_measured: pose.position_tracked,
            orientation_measured: pose.orientation_tracked,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Provenance {
    pub sample: SampleStamp,
    pub previous: StepStamp,
    pub current: StepStamp,
    pub support: Option<(u64, u64)>,
    pub transition: Transition,
    pub fresh_measurement: bool,
}

/// The nominal support destination, distinct from posture-dependent pelvis motion.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Stance {
    pub origin: Vec3,
    pub yaw: f32,
}

/// World-space inferred targets. The torso has two fixed-length segments.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Body {
    pub pelvis: Vec3,
    pub pelvis_rotation: Quat,
    pub chest: Vec3,
    pub neck: Vec3,
    pub shoulders: [Vec3; 2],
    pub hips: [Vec3; 2],
    pub knees: [Vec3; 2],
    pub ankles: [Vec3; 2],
    pub chest_yaw: f32,
    pub hip_flexion: f32,
    pub spine_flexion: f32,
    pub knee_flexion: [f32; 2],
    pub ankle_flexion: [f32; 2],
    pub neck_twist: f32,
}

/// No field in this request credits movement. The movement owner must admit and complete it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RootRequest {
    pub displacement: Vec3,
    pub turn: f32,
    pub permitted: bool,
}

/// Gait phase receives completed root travel relative to the observed support, never pelvis sway.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct GaitOutput {
    pub completed_across_support: Vec3,
    pub distance: f32,
    pub phase: f32,
    pub touchdown_hips: [Vec3; 2],
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Quality {
    pub head: TrackingQuality,
    pub hands: [Option<TrackingQuality>; 2],
    pub neck_residual_m: f32,
    pub hand_reach_residual_m: [Option<f32>; 2],
    pub neck_limit_residual_rad: f32,
    pub contact_residual_m: [f32; 2],
    pub feasible: bool,
    pub reduced_authority: bool,
    pub yaw_confidence: Confidence,
    /// Fraction of tested chest-heading offsets with an admissible stance/twist allocation.
    pub yaw_ambiguity: f32,
    pub lower_body_confidence: Confidence,
    pub evaluations: usize,
    pub search: fit::SearchStatus,
    pub temporal: temporal::TemporalStatus,
}

/// Immutable result of one commit. A getter does not advance any state or evidence count.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Snapshot {
    pub provenance: Provenance,
    pub stance: Stance,
    pub body: Body,
    pub player_shoulders: [Vec3; 2],
    pub remapped_hands: [Option<Vec3>; 2],
    pub feet: [FootPose; 2],
    pub contacts: [ContactState; 2],
    pub root: RootRequest,
    pub gait: GaitOutput,
    pub quality: Quality,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum EstimateError {
    Observation(ObservationError),
    InvalidAnatomy,
    InvalidSupport,
    SupportPairMismatch,
    InvalidTimeStep,
    InvalidRoot,
    EpochRequiresReseed,
    UnexpectedRecenter,
    ReversedSample,
    ReversedStep,
    ChangedCommittedStep,
    MissedStep,
    Ownership,
    NoFiniteCandidate,
    SearchExhausted,
    EmptyTemporalIntersection { axis: usize },
}

impl fmt::Display for EstimateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cause = match self {
            Self::Observation(error) => return write!(f, "body estimate: {error}"),
            Self::InvalidAnatomy => "the proportions or reference rig are unusable",
            Self::InvalidSupport => "the named support pair is invalid or does not match the step",
            Self::SupportPairMismatch => {
                "the support pair does not continue the committed transform"
            }
            Self::InvalidTimeStep => "the simulation interval is outside the bounded fit interval",
            Self::InvalidRoot => "the root is not a rigid upright transform",
            Self::EpochRequiresReseed => {
                "the calibration, scale, or authority changes without a reseed"
            }
            Self::UnexpectedRecenter => "the origin changes without an explicit recenter or reseed",
            Self::ReversedSample => "the tracking sequence or display time runs backward",
            Self::ReversedStep => "the simulation step runs backward",
            Self::ChangedCommittedStep => "a committed step is submitted with different inputs",
            Self::MissedStep => "a simulation step is missing; an explicit reseed is required",
            Self::Ownership => "the observed owner does not permit a standing shadow fit",
            Self::NoFiniteCandidate => "a required finite target is unavailable",
            Self::SearchExhausted => {
                "the bounded search exhausts its represented seeds; geometric infeasibility is not established"
            }
            Self::EmptyTemporalIntersection { axis } => {
                return write!(
                    f,
                    "body estimate: temporal and anatomical bounds have no intersection on coordinate {axis}"
                );
            }
        };
        write!(f, "body estimate: {cause}")
    }
}

impl Error for EstimateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Observation(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ObservationError> for EstimateError {
    fn from(error: ObservationError) -> Self {
        Self::Observation(error)
    }
}

/// Two four-topology passes, four fixed-pose recovery checks, one realization, and twelve probes.
pub(crate) const MAX_EVALUATIONS: usize = 8 * fit::SEARCH_EVALUATIONS + 4 + 1 + fit::HEADING_PROBES;

#[derive(Debug, Copy, Clone, PartialEq)]
struct Temporal {
    parameters: fit::Parameters,
    rates: [f32; fit::PARAMETERS],
    contacts: [Contact; 2],
    snapshot: Snapshot,
}
