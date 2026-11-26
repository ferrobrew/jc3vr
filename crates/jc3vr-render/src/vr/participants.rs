//! The gameplay work the VR frame drives without owning it: the roomscale cockpit composition, the
//! composition's evidence publication, and the observers of a recenter. The grapple's body-frame
//! filter, which the frame also advances, reaches it through the head pose's participants
//! ([`crate::headpose::HeadParticipants`]).
//!
//! The frame ([`crate::vr::frame`]) and the recenter ([`crate::vr::recenter`]) call these at fixed
//! points of their own sequence, so the order is theirs. The owners live above the VR runtime, so
//! they hand it one [`FrameParticipants`] table at install ([`register_frame_participants`]),
//! before any hook can start a frame.

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

static PARTICIPANTS: OnceLock<&'static FrameParticipants> = OnceLock::new();
