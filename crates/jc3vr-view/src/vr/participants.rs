//! The work the VR runtime drives without owning it: the roomscale cockpit composition, the
//! observer of a recenter, and the kind a manual recenter asks for; the flip state its teardown
//! releases; and the observer of the engine resizes it requests. The body frame filter, which the
//! frame also advances, reaches it through the head pose's participants
//! ([`crate::headpose::BodyFrameFilter`]).
//!
//! The frame ([`crate::vr::frame`]), the recenter ([`mod@crate::vr::recenter`]), the teardown, and the
//! native-resolution driver call these at fixed points of their own sequence, so the order is
//! theirs. The owners live above the VR runtime, so they hand it their tables at install
//! ([`register_frame_participants`], [`register_teardown_participant`], and
//! [`register_resize_observer`]), before any hook can start a frame.

use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use glam::{Quat, Vec3};

use crate::vr::{RecenterCause, RecenterKind};

/// The functions the VR frame and the recenter call into their owners through. Each runs on the
/// game thread, at the point of the frame its caller documents.
pub struct FrameParticipants {
    /// Resolve the posture, remap the head height, and shift the cockpit for the frame, returning
    /// the cockpit positions to compose the pose pair with.
    pub compose_cockpit: fn(inputs: &FrameInputs) -> CockpitPositions,
    /// Follow a re-base of the cockpit baseline: the cockpit frame turned by `turn`, new frame from
    /// old.
    pub on_recenter: fn(kind: RecenterKind, turn: Quat, cause: RecenterCause),
    /// The recenter kind a manual recenter asks for in the current posture.
    pub manual_recenter_kind: fn() -> RecenterKind,
}

/// One rendered frame's inputs to the pose composition.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FrameInputs {
    /// The head's cockpit position (relative to the recenter baseline), metres.
    pub cockpit: Vec3,
    /// The head's cockpit orientation.
    pub orientation: Quat,
    /// The body rotation the cockpit composes with.
    pub body: Quat,
    pub world_scale: f32,
}

/// The cockpit positions to compose the two sides of the pose pair with.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CockpitPositions {
    pub prev: Vec3,
    pub cur: Vec3,
}

/// The render-side state the VR runtime's teardown releases, on the game thread.
pub struct TeardownParticipant {
    /// Release the flip block, so the game's own present path resumes. Runs whenever the runtime
    /// tears down, after the GPU is drained.
    pub release_flip: fn(),
}

/// The observer of the engine resizes the native-resolution driver requests. Both run on the game
/// thread.
pub struct ResizeObserver {
    /// The engine is initialized and the driver is about to inspect its display size. Runs every
    /// frame the driver runs against an initialized engine.
    pub engine_ready: fn(),
    /// The engine serviced a requested resize to `width` by `height`.
    pub resized: fn(width: u32, height: u32),
}

/// Hand the VR frame its participants. Called once at install; a second table is ignored.
pub fn register_frame_participants(participants: &'static FrameParticipants) {
    if PARTICIPANTS.set(participants).is_err() {
        tracing::warn!(
            target: "vr",
            "frame participants: a table is already registered; the new one is ignored"
        );
    }
}

/// Whether the VR frame's participants are registered.
pub fn frame_participants_registered() -> bool {
    PARTICIPANTS.get().is_some()
}

/// The registered participants, or `None` when no table is registered. Every frame and recenter
/// runs from a hook, and the startup's registration check refuses a composition without the table,
/// so a missing one is logged once and the caller skips the work that needs it.
pub(crate) fn frame_participants() -> Option<&'static FrameParticipants> {
    let participants = PARTICIPANTS.get().copied();
    if participants.is_none() && !FRAME_MISSING_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::error!(target: "vr", "frame participants: no table is registered; the frame stands down");
    }
    participants
}

/// Hand the VR teardown its participant. Called once at install; a second one is ignored.
pub fn register_teardown_participant(participant: &'static TeardownParticipant) {
    if TEARDOWN.set(participant).is_err() {
        tracing::warn!(
            target: "vr",
            "teardown participant: one is already registered; the new one is ignored"
        );
    }
}

/// Whether the VR teardown's participant is registered.
pub fn teardown_participant_registered() -> bool {
    TEARDOWN.get().is_some()
}

/// The registered teardown participant, or `None` when none is registered. The teardown releases
/// state the runtime cannot release itself, so a runtime without it would leave the flip blocked;
/// the startup's registration check refuses a composition without it, and a missing one is logged
/// once.
pub(crate) fn teardown_participant() -> Option<&'static TeardownParticipant> {
    let participant = TEARDOWN.get().copied();
    if participant.is_none() && !TEARDOWN_MISSING_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::error!(target: "vr", "teardown participant: none is registered; the flip stays blocked");
    }
    participant
}

/// Hand the native-resolution driver its resize observer. Called once at install; a second one is
/// ignored.
pub fn register_resize_observer(observer: &'static ResizeObserver) {
    if RESIZE_OBSERVER.set(observer).is_err() {
        tracing::warn!(
            target: "vr",
            "resize observer: one is already registered; the new one is ignored"
        );
    }
}

/// Whether the resize observer is registered.
pub fn resize_observer_registered() -> bool {
    RESIZE_OBSERVER.get().is_some()
}

/// The registered resize observer, or `None` when nobody observes the resizes.
pub(crate) fn resize_observer() -> Option<&'static ResizeObserver> {
    RESIZE_OBSERVER.get().copied()
}

static FRAME_MISSING_LOGGED: AtomicBool = AtomicBool::new(false);

static TEARDOWN_MISSING_LOGGED: AtomicBool = AtomicBool::new(false);

static PARTICIPANTS: OnceLock<&'static FrameParticipants> = OnceLock::new();

static TEARDOWN: OnceLock<&'static TeardownParticipant> = OnceLock::new();

static RESIZE_OBSERVER: OnceLock<&'static ResizeObserver> = OnceLock::new();
