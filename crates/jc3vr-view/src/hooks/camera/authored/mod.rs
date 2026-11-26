//! Following the game's authored camera through the upside-down grapple.
//!
//! The grapple's upside-down states, and the stunt hang that shares their idle clip, animate the
//! `CharacterCam1` marker beyond the clip's own root motion. Under the flatscreen head source the
//! view follows that marker relative to its pose at entry, composed under the tracked head, and
//! eases back to the normal tracked view once the clips stop contributing. The native model-space
//! channel retains its Reference hierarchy; the character world root remains in the view base.

mod composition;
mod evidence;
mod native;
mod runtime;

pub use evidence::select_controller;
pub use native::{animation_evaluation, animation_update};
use parking_lot::Mutex;
pub(crate) use runtime::{TrackedView, capture, normal_reset, reset, selected, take_normal_reset};

/// The camera debug UI displays the current state of the authored camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    WaitingForCameraTree,
    NormalTracked { reason: Refusal },
    Following { phase: Phase },
    Handback { reason: Refusal },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The first sample of a run, which only records the entry baseline.
    Entry,
    Live,
    /// The state has ended and its clips are still blending out.
    Tail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Disabled,
    OutsideGameplay,
    NoTrackedHead,
    HeadsetSource,
    MissingState,
    UnsupportedState,
    MissingChannel,
    MissingClip,
    MissingFinalizedAnimation,
    IncoherentAnimation,
    InvalidClipTime,
    InvalidNativeWeight,
    NativeTailComplete,
    InvalidTransform,
    CharacterChanged,
    SourceChanged,
    Teleport,
}

pub fn status() -> Status {
    *STATUS.lock()
}

static STATUS: Mutex<Status> = Mutex::new(Status::WaitingForCameraTree);
