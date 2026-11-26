//! Pure ownership for explicitly named authored-camera runs.

use crate::hooks::camera::authored::composition::{
    Baseline, DeltaRequest, InvalidPair, Pair, RootTreatment, Sample, Stamp, check_root_semantics,
    same_continuity,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NamedCase {
    #[cfg(test)]
    BoostedGroundTakeoff,
    #[cfg(test)]
    Landing,
    #[cfg(test)]
    Recovery,
    #[cfg(test)]
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

/// The graph states the policy distinguishes. The classifier admits only the upside-down grapple
/// family (see `native::EXACT_STATES`). The takeoff, landing, recovery, pickup, and reeled-in
/// attach cases exist only in tests, as fixtures for the run, handoff, and tail rules: their
/// shipped markers only cancel root motion or hold identity, so the classifier never admits them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GraphState {
    #[cfg(test)]
    GroundInto,
    #[cfg(test)]
    GroundFollowOn,
    #[cfg(test)]
    WingsuitIdle,
    #[cfg(test)]
    Landing(LandingState),
    #[cfg(test)]
    Recovery(RecoveryState),
    #[cfg(test)]
    FloorPickup,
    Hanging(HangingState),
    #[cfg(test)]
    OrdinaryLocomotion,
    #[cfg(test)]
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
pub(crate) enum LandingState {
    FastFall,
    Hard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
pub(crate) enum RecoveryState {
    PoseMatching,
    BlendIn,
    GetUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HangingState {
    #[cfg(test)]
    GrappleAttached,
    GrapplePreUpsideDown,
    GrappleUpsideDown,
    StuntIdle,
}

impl GraphState {
    pub fn named_case(self) -> Option<NamedCase> {
        match self {
            #[cfg(test)]
            Self::GroundInto | Self::GroundFollowOn | Self::WingsuitIdle => {
                Some(NamedCase::BoostedGroundTakeoff)
            }
            #[cfg(test)]
            Self::Landing(_) => Some(NamedCase::Landing),
            #[cfg(test)]
            Self::Recovery(_) => Some(NamedCase::Recovery),
            #[cfg(test)]
            Self::FloorPickup => Some(NamedCase::FloorPickup),
            Self::Hanging(_) => Some(NamedCase::Hanging),
            #[cfg(test)]
            Self::OrdinaryLocomotion | Self::Other => None,
        }
    }

    pub fn can_enter(self) -> bool {
        #[cfg(test)]
        if matches!(self, Self::GroundFollowOn | Self::WingsuitIdle) {
            return false;
        }
        self.named_case().is_some()
    }

    /// Only explicit graph handoffs share an entry baseline across distinct motions.
    pub fn continues(self, next: Self) -> bool {
        if self.named_case().is_none() {
            return false;
        }
        self == next || self.hands_off_to(next)
    }

    fn hands_off_to(self, next: Self) -> bool {
        match (self, next) {
            #[cfg(test)]
            (Self::GroundInto, Self::GroundFollowOn)
            | (Self::GroundFollowOn, Self::WingsuitIdle)
            | (
                Self::Recovery(RecoveryState::PoseMatching),
                Self::Recovery(RecoveryState::BlendIn | RecoveryState::GetUp),
            )
            | (Self::Recovery(RecoveryState::BlendIn), Self::Recovery(RecoveryState::GetUp)) => {
                true
            }
            (
                Self::Hanging(HangingState::GrapplePreUpsideDown),
                Self::Hanging(HangingState::GrappleUpsideDown),
            ) => true,
            _ => false,
        }
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

/// Presence of the admitted clip set in this serial's native camera tracks.
/// This is not a normalized weight and never scales an already blended pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NativeContribution {
    pub stamp: Stamp,
    pub run: u64,
    pub source: u64,
    pub outgoing: bool,
    pub contributing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    pub accepted: AcceptedRun,
    pub state: GraphState,
    pub sample: Sample,
    pub baseline: Baseline,
    pub blend: Option<FinalizedBlend>,
    pub native: Option<NativeContribution>,
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

/// Native contribution gates ownership; pose composition retains the native blend once.
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
        if frame.blend.is_some_and(|blend| blend.weight == 0.0)
            || frame
                .native
                .is_some_and(|native| native.outgoing && !native.contributing)
        {
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
        let tail = frame.native.is_some_and(|native| native.outgoing);
        #[cfg(test)]
        let tail = tail || frame.state == GraphState::WingsuitIdle;
        Decision {
            phase: if tail { Phase::Tail } else { Phase::Live },
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
    if let Some(native) = frame.native {
        if native.stamp != stamp {
            return Err(InvalidPair::BlendEpochMismatch);
        }
        if native.run != frame.accepted.run || native.source != frame.source {
            return Err(InvalidPair::SourceChanged);
        }
        if frame.clip_time.is_none() {
            return Err(InvalidPair::MissingSample);
        }
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
    }
    #[cfg(test)]
    if frame.blend.is_none() && frame.state == GraphState::WingsuitIdle {
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
    check_root_semantics(frame.request, frame.root_treatment)
}

fn continue_pair(previous: Frame, current: Frame) -> Result<Pair, InvalidPair> {
    same_continuity(previous.sample.stamp()?, current.sample.stamp()?)?;
    if previous.accepted != current.accepted {
        return Err(InvalidPair::AdmissionMismatch);
    }
    if previous.baseline != current.baseline {
        return Err(InvalidPair::BaselineChanged);
    }
    let native_handoff = previous.native.is_some()
        && current.native.is_some()
        && previous.state != current.state
        && previous.state.continues(current.state);
    if (previous.source != current.source && !native_handoff)
        || previous.request != current.request
        || previous.root_treatment != current.root_treatment
    {
        return Err(InvalidPair::SourceChanged);
    }
    match (previous.clip_time, current.clip_time) {
        (Some(before), Some(after)) if after < before && current.native.is_none() => {
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
