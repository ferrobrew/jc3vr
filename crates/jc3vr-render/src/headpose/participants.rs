//! The gameplay state and work the head pose reads without owning it: the grapple's body-frame
//! comfort filter and its telemetry, and the character's state that gates the body turn and the
//! anchors' continuity.
//!
//! The head composition calls these at fixed points of its own sequence, on the input tick and
//! on every rendered frame. Their owners live above the head pose, so they hand it one
//! [`HeadParticipants`] table at install ([`register_head_participants`]), before any hook can
//! tick the simulation or start a frame.

use std::sync::OnceLock;

use glam::{Quat, Vec3};

use crate::headpose::{config::GrappleComfortConfig, xr::FollowSettings};

/// The functions the head pose calls into its owners through.
pub struct HeadParticipants {
    /// Advance the grapple's body-frame filter: the body rotation the view composes with, the
    /// view head's offset from the character's root (`None` while the pose is frozen or without
    /// anchors), and whether the head follows the character on foot.
    pub advance_grapple:
        fn(body: Quat, rooted: Option<Vec3>, on_foot: bool, config: &GrappleComfortConfig),
    /// Apply the grapple filter's body-frame hold to a body rotation.
    pub filter_body_rotation: fn(body: Quat) -> Quat,
    /// Pass the head anchor through the grapple filter's landing rate limit.
    pub filter_anchor: fn(anchor: Vec3) -> Vec3,
    /// The grapple filter's blend, from zero (off) to one.
    pub grapple_blend: fn() -> f32,
    /// Take the heading the grapple's yaw handoff wants the body turned toward, if still fresh.
    pub take_body_yaw_retarget: fn() -> Option<f32>,
    /// Write one input tick's row of the grapple telemetry capture.
    pub log_grapple_tick: fn(body: Quat, dt: f32),
    /// Write one rendered frame's row of the grapple telemetry capture.
    pub log_grapple_frame: fn(frame: &FrameTelemetry),
    /// How the body follows the head this tick.
    pub follow_settings: fn() -> FollowSettings,
    /// The on-foot orientation evaluator's run count, which advances once per tick on foot.
    pub orientation_evals: fn() -> u64,
    /// The swim movement cores' run count.
    pub swim_evals: fn() -> u64,
    /// Whether the local player swims.
    pub swimming: fn() -> bool,
    /// Whether a seat transition is entering or leaving a vehicle.
    pub vehicle_transition: fn() -> bool,
    /// Whether the body is thrown: ragdolling, getting up, or dead.
    pub body_thrown: fn() -> bool,
    /// Whether the controllers' mode is the UI rather than gameplay.
    pub ui_mode: fn() -> bool,
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

/// Hand the head pose its participants. Called once at install; a second table is ignored.
pub fn register_head_participants(participants: &'static HeadParticipants) {
    if PARTICIPANTS.set(participants).is_err() {
        tracing::warn!("head participants: a table is already registered; the new one is ignored");
    }
}

/// The registered participants. The simulation ticks and the frames run from hooks, and the table
/// is registered before the hooks are installed, so it is always present by then.
pub(crate) fn head_participants() -> &'static HeadParticipants {
    PARTICIPANTS
        .get()
        .expect("head participants: the table is registered before the hooks are installed")
}

static PARTICIPANTS: OnceLock<&'static HeadParticipants> = OnceLock::new();
