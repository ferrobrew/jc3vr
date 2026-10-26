//! Selective native camera-channel following for controlled gameplay testing.
//!
//! Only the exact boosted ground-takeoff states admit the adapter. The native model-space
//! channel retains its Reference hierarchy; the character world root remains in the view base.

mod composition;
mod native;
mod policy;
mod runtime;
#[cfg(test)]
mod tests;

use parking_lot::Mutex;
pub(crate) use runtime::{TrackedView, capture, normal_reset, reset, selected, take_normal_reset};

/// The existing camera debug UI displays the current admission and its integration limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    WaitingForCameraTree,
    NormalTracked {
        reason: Refusal,
        unsupported: UnsupportedCases,
    },
    BoostedGroundTakeoff {
        phase: policy::Phase,
        limitation: Limit,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    Disabled,
    OutsideGameplay,
    NoTrackedHead,
    MissingState,
    OrdinaryOrUnsupportedState,
    PickupNotVerified,
    HangingNotVerified,
    IdleTailWeightNotBound,
    MissingChannel,
    InvalidTransform,
    SourceChanged,
    Policy(composition::InvalidPair),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Limit {
    ExactStatesOnlyTreeSampledNoBlendTail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnsupportedCases {
    LandingExplosionRecoveryFloorPickupHanging,
}

pub(crate) fn status() -> Status {
    *STATUS.lock()
}

static STATUS: Mutex<Status> = Mutex::new(Status::WaitingForCameraTree);
