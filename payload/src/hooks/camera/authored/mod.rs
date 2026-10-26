//! Selective native camera-channel following for controlled gameplay testing.
//!
//! Exact named takeoff, landing, recovery, pickup, and hanging states admit the adapter.
//! The native model-space channel retains its Reference hierarchy; the character world
//! root remains in the view base.

mod composition;
mod evidence;
mod native;
mod policy;
mod runtime;
#[cfg(test)]
mod tests;

pub(crate) use native::{animation_evaluation, animation_update};
use parking_lot::Mutex;
pub(crate) use runtime::{TrackedView, capture, normal_reset, reset, selected, take_normal_reset};

/// The existing camera debug UI displays the current admission and its integration limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    WaitingForCameraTree,
    NormalTracked {
        reason: Refusal,
    },
    NamedCase {
        case: policy::NamedCase,
        phase: policy::Phase,
        limitation: Limit,
    },
    Handback {
        case: policy::NamedCase,
        reason: Refusal,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    Disabled,
    OutsideGameplay,
    NoTrackedHead,
    MissingState,
    OrdinaryOrUnsupportedState,
    IdleTailWeightNotBound,
    MissingChannel,
    MissingClip,
    MissingFinalizedAnimation,
    IncoherentAnimation,
    InvalidClipTime,
    InvalidNativeWeight,
    NativeTailComplete,
    InvalidTransform,
    Policy(composition::InvalidPair),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Limit {
    SynchronousSourceVerifiedNativeBlend,
}

pub(crate) fn status() -> Status {
    *STATUS.lock()
}

static STATUS: Mutex<Status> = Mutex::new(Status::WaitingForCameraTree);
