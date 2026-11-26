//! The work the VR runtime drives without owning it: the roomscale cockpit composition, the
//! composition's evidence publication, and the observers of a recenter; the render resources and
//! flip state its teardown releases; and the observer of the engine resizes it requests. The
//! grapple's body-frame filter, which the frame also advances, reaches it through the head pose's
//! participants ([`crate::headpose::HeadParticipants`]).
//!
//! The frame ([`crate::vr::frame`]), the recenter ([`crate::vr::recenter`]), the teardown, and the
//! native-resolution driver call these at fixed points of their own sequence, so the order is
//! theirs. The owners live above the VR runtime, so they hand it their tables at install
//! ([`register_frame_participants`], [`register_teardown_participant`], and
//! [`register_resize_observer`]), before any hook can start a frame.

use std::sync::OnceLock;

use glam::{Quat, Vec3};

use crate::{
    controllers::body::rig::origin_provenance::{OriginEvidence, TrackingReceipt},
    vr::{RecenterCause, RecenterKind},
};

/// The functions the VR frame and the recenter call into their owners through. Each runs on the
/// game thread, at the point of the frame its caller documents.
pub struct FrameParticipants {
    /// Resolve the posture, remap the head height, and shift the cockpit for the frame, returning
    /// the cockpit positions to compose the pose pair with.
    pub compose_cockpit: fn(inputs: &FrameInputs) -> CockpitPositions,
    /// Publish the evidence of the composition [`compose_cockpit`](Self::compose_cockpit) last
    /// returned, with the live tracking origin it composed, or clear it with `None` when no live,
    /// unfrozen pose pair is published.
    pub publish_composition: fn(tracking: Option<(OriginEvidence, Option<TrackingReceipt>)>),
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

/// The registered participants. Every frame and recenter runs from a hook, and the table is
/// registered before the hooks are installed, so it is always present by then.
pub(crate) fn frame_participants() -> &'static FrameParticipants {
    PARTICIPANTS
        .get()
        .expect("frame participants: the table is registered before the hooks are installed")
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

/// The registered teardown participant. The teardown releases state the runtime cannot release
/// itself, so a runtime without it would leave the flip blocked: [`crate::vr::install`] checks it is
/// present before the runtime can bring a session up.
pub(crate) fn teardown_participant() -> &'static TeardownParticipant {
    TEARDOWN
        .get()
        .expect("teardown participant: registered before the VR runtime is installed")
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

/// The registered resize observer, or `None` when nobody observes the resizes.
pub(crate) fn resize_observer() -> Option<&'static ResizeObserver> {
    RESIZE_OBSERVER.get().copied()
}

static PARTICIPANTS: OnceLock<&'static FrameParticipants> = OnceLock::new();

static TEARDOWN: OnceLock<&'static TeardownParticipant> = OnceLock::new();

static RESIZE_OBSERVER: OnceLock<&'static ResizeObserver> = OnceLock::new();
