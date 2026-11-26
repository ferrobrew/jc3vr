//! The gameplay work the VR frame drives without owning it: the grapple's body-frame filter, the
//! roomscale cockpit composition, the composition's evidence publication, the grapple telemetry
//! row, and the observers of a recenter.
//!
//! The frame ([`crate::vr::frame`]) and the recenter ([`crate::vr::recenter`]) call these at fixed
//! points of their own sequence, so the order is theirs. The owners live above the VR runtime, so
//! they hand it one [`FrameParticipants`] table at install ([`register_frame_participants`]),
//! before any hook can start a frame.

use std::sync::OnceLock;

use glam::{Quat, Vec3};

use crate::{
    controllers::body::rig::origin_provenance::{OriginEvidence, TrackingReceipt},
    headpose::config::GrappleComfortConfig,
    vr::{RecenterCause, RecenterKind},
};

/// The functions the VR frame and the recenter call into their owners through. Each runs on the
/// game thread, at the point of the frame its caller documents.
pub(crate) struct FrameParticipants {
    /// Advance the grapple reel-in filter: the body rotation the view composes with, the view
    /// head's offset from the character's root (`None` while the pose is frozen), and whether the
    /// head follows the character on foot.
    pub advance_grapple:
        fn(body: Quat, rooted: Option<Vec3>, on_foot: bool, config: &GrappleComfortConfig),
    /// Pass the head anchor through the grapple filter's landing rate limit.
    pub filter_anchor: fn(anchor: Vec3) -> Vec3,
    /// Resolve the posture, remap the head height, and shift the cockpit for the frame, returning
    /// the cockpit positions to compose the pose pair with.
    pub compose_cockpit: fn(inputs: &FrameInputs) -> CockpitPositions,
    /// Publish the evidence of the composition [`compose_cockpit`](Self::compose_cockpit) last
    /// returned, with the live tracking origin it composed, or clear it with `None` when no live,
    /// unfrozen pose pair is published.
    pub publish_composition: fn(tracking: Option<(OriginEvidence, Option<TrackingReceipt>)>),
    /// Write one rendered frame's row of the grapple telemetry capture.
    pub log_grapple_frame: fn(frame: &FrameTelemetry),
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
pub(crate) struct CockpitPositions {
    pub prev: Vec3,
    pub cur: Vec3,
}

/// One rendered VR frame's pose composition, for the grapple telemetry's frame row: the HMD cockpit
/// pose, the raw and filtered body frames the frame used, and the composed head pose it published.
pub struct FrameTelemetry {
    pub cockpit_orientation: Quat,
    pub cockpit_position: Vec3,
    pub body_raw: Quat,
    pub body_filtered: Quat,
    pub composed: Quat,
    pub position: Vec3,
    pub anchor: Vec3,
}

/// Hand the VR frame its participants. Called once at install; a second table is ignored.
pub(crate) fn register_frame_participants(participants: &'static FrameParticipants) {
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
