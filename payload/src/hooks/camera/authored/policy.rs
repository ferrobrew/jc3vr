//! Pure ownership for explicitly named authored-camera runs.

use crate::hooks::camera::authored::composition::{
    Baseline, DeltaRequest, InvalidPair, Pair, RootTreatment, Sample, Stamp, same_continuity,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NamedCase {
    BoostedGroundTakeoff,
    Landing,
    Recovery,
    FloorPickup,
    Hanging,
}

/// An admission is supplied explicitly; a broad predicate cannot construct one implicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AcceptedRun {
    pub case: NamedCase,
    pub character: u64,
    pub continuity: u64,
    pub run: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GraphState {
    GroundInto,
    GroundFollowOn,
    WingsuitIdle,
    Landing(LandingState),
    Recovery(RecoveryState),
    FloorPickup,
    Hanging(HangingState),
    #[cfg(test)]
    OrdinaryLocomotion,
    #[cfg(test)]
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LandingState {
    FastFall,
    Hard,
    Idle,
    IdleLeft,
    IdleRight,
    IdleBackLeft,
    IdleBackRight,
    Run,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecoveryState {
    PoseMatching,
    BlendIn,
    GetUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HangingState {
    GrappleAttached,
    StuntIdle,
    StuntIdleLeft,
    StuntIdleRight,
    StuntIdleAlternate,
}

impl GraphState {
    pub fn named_case(self) -> Option<NamedCase> {
        match self {
            Self::GroundInto | Self::GroundFollowOn | Self::WingsuitIdle => {
                Some(NamedCase::BoostedGroundTakeoff)
            }
            Self::Landing(_) => Some(NamedCase::Landing),
            Self::Recovery(_) => Some(NamedCase::Recovery),
            Self::FloorPickup => Some(NamedCase::FloorPickup),
            Self::Hanging(_) => Some(NamedCase::Hanging),
            #[cfg(test)]
            Self::OrdinaryLocomotion | Self::Other => None,
        }
    }

    pub fn can_enter(self) -> bool {
        !matches!(self, Self::GroundFollowOn | Self::WingsuitIdle) && self.named_case().is_some()
    }

    /// Distinct landing and hanging motions do not share an entry baseline.
    pub fn continues(self, next: Self) -> bool {
        if self.named_case().is_none() {
            return false;
        }
        self == next
            || matches!(
                (self, next),
                (Self::GroundInto, Self::GroundFollowOn)
                    | (Self::GroundFollowOn, Self::WingsuitIdle)
                    | (
                        Self::Recovery(RecoveryState::PoseMatching),
                        Self::Recovery(RecoveryState::BlendIn | RecoveryState::GetUp)
                    )
                    | (
                        Self::Recovery(RecoveryState::BlendIn),
                        Self::Recovery(RecoveryState::GetUp)
                    )
            )
    }
}

/// The weight identifies the accepted animation's contribution to this finalized pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FinalizedBlend {
    pub stamp: Stamp,
    pub run: u64,
    pub source: u64,
    pub weight: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    pub accepted: AcceptedRun,
    pub state: GraphState,
    pub sample: Sample,
    pub baseline: Baseline,
    pub blend: Option<FinalizedBlend>,
    pub source: u64,
    pub clip_time: Option<f32>,
    pub request: DeltaRequest,
    pub root_treatment: RootTreatment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Idle,
    Entry,
    Live,
    Tail,
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    NormalTracked,
    Authored,
}

/// Only a valid pair grants ownership. A reset never pauses the normal tracked owner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Decision {
    pub phase: Phase,
    pub pair: Option<Pair>,
    pub reset: bool,
    pub reason: Option<InvalidPair>,
}

impl Decision {
    pub fn owner(self) -> Owner {
        if self.pair.is_some() {
            Owner::Authored
        } else {
            Owner::NormalTracked
        }
    }
}

/// The native exact-state reader omits clip time and weight rather than inventing them.
/// Such a reader can own only named states, never an outgoing blend tail.
#[derive(Default)]
pub(crate) struct Policy {
    previous: Option<Frame>,
}

impl Policy {
    pub const fn new() -> Self {
        Self { previous: None }
    }

    #[cfg(test)]
    pub fn unavailable(&mut self) -> Decision {
        self.clear(Phase::Idle, Some(InvalidPair::MissingSample))
    }

    pub fn teleport(&mut self) -> Decision {
        self.clear(Phase::Interrupted, Some(InvalidPair::Teleport))
    }

    pub fn interrupt(&mut self) -> Decision {
        self.clear(Phase::Interrupted, Some(InvalidPair::Interrupted))
    }

    pub fn observe(&mut self, frame: Frame) -> Decision {
        #[cfg(test)]
        if matches!(
            frame.state,
            GraphState::OrdinaryLocomotion | GraphState::Other
        ) {
            return self.clear(Phase::Interrupted, Some(InvalidPair::UnsupportedState));
        }
        if let Err(reason) = validate(frame) {
            return self.clear(Phase::Interrupted, Some(reason));
        }
        if frame.blend.is_some_and(|blend| blend.weight == 0.0) {
            return self.clear(Phase::Idle, None);
        }
        let Some(previous) = self.previous else {
            if !frame.state.can_enter() {
                return self.clear(Phase::Idle, Some(InvalidPair::InvalidTransition));
            }
            self.previous = Some(frame);
            return Decision {
                phase: Phase::Entry,
                pair: None,
                reset: true,
                reason: None,
            };
        };
        let pair = match continue_pair(previous, frame) {
            Ok(pair) => pair,
            Err(reason) => return self.clear(Phase::Interrupted, Some(reason)),
        };
        self.previous = Some(frame);
        Decision {
            phase: if frame.state == GraphState::WingsuitIdle {
                Phase::Tail
            } else {
                Phase::Live
            },
            pair: Some(pair),
            reset: false,
            reason: None,
        }
    }

    fn clear(&mut self, phase: Phase, reason: Option<InvalidPair>) -> Decision {
        self.previous = None;
        Decision {
            phase,
            pair: None,
            reset: true,
            reason,
        }
    }
}

fn validate(frame: Frame) -> Result<(), InvalidPair> {
    let stamp = frame.sample.stamp()?;
    let origin = frame.baseline.sample.stamp()?;
    same_continuity(stamp, origin)?;
    if frame.state.named_case() != Some(frame.accepted.case)
        || frame.accepted.character != stamp.character
        || frame.accepted.continuity != stamp.continuity
        || frame.accepted.run != frame.baseline.run
    {
        return Err(InvalidPair::AdmissionMismatch);
    }
    if let Some(blend) = frame.blend {
        if frame.accepted.run != blend.run {
            return Err(InvalidPair::AdmissionMismatch);
        }
        if blend.stamp != stamp {
            return Err(InvalidPair::BlendEpochMismatch);
        }
        if blend.source != frame.source {
            return Err(InvalidPair::SourceChanged);
        }
        if !blend.weight.is_finite() || !(0.0..=1.0).contains(&blend.weight) {
            return Err(InvalidPair::InvalidBlendWeight);
        }
    } else if frame.state == GraphState::WingsuitIdle {
        return Err(InvalidPair::MissingSample);
    }
    if frame
        .clip_time
        .is_some_and(|time| !time.is_finite() || time < 0.0)
    {
        return Err(InvalidPair::ClipTimeReversed);
    }
    if origin.finalized > stamp.finalized {
        return Err(InvalidPair::RepeatedOrReversedSample);
    }
    match (frame.request, frame.root_treatment) {
        (_, RootTreatment::Unknown) => Err(InvalidPair::RootTreatmentUnknown),
        (
            DeltaRequest::AuthoredLocalRelativeToBaseline,
            RootTreatment::SeparateFromAuthoredLocal,
        )
        | (
            DeltaRequest::RootCompensatedRelativeToBaseline,
            RootTreatment::ComposeWithAuthoredLocal,
        ) => Ok(()),
        _ => Err(InvalidPair::RootTreatmentMismatch),
    }
}

fn continue_pair(previous: Frame, current: Frame) -> Result<Pair, InvalidPair> {
    same_continuity(previous.sample.stamp()?, current.sample.stamp()?)?;
    if previous.accepted != current.accepted {
        return Err(InvalidPair::AdmissionMismatch);
    }
    if previous.baseline != current.baseline {
        return Err(InvalidPair::BaselineChanged);
    }
    if previous.source != current.source
        || previous.request != current.request
        || previous.root_treatment != current.root_treatment
    {
        return Err(InvalidPair::SourceChanged);
    }
    match (previous.clip_time, current.clip_time) {
        (Some(before), Some(after)) if after < before => {
            return Err(InvalidPair::ClipTimeReversed);
        }
        (Some(_), None) | (None, Some(_)) => return Err(InvalidPair::SourceChanged),
        _ => {}
    }
    if previous.blend.is_some() != current.blend.is_some() {
        return Err(InvalidPair::SourceChanged);
    }
    if !previous.state.continues(current.state) {
        return Err(InvalidPair::InvalidTransition);
    }
    let pair = Pair::new(previous.sample, current.sample, current.baseline)?;
    pair.deltas(current.request, current.root_treatment)?;
    Ok(pair)
}
