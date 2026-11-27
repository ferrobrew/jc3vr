//! The gameplay state and work the head pose reads without owning it: a filter on the body frame
//! and anchor the head composes onto, and the character's state that gates the body turn and the
//! anchors' continuity.
//!
//! The head composition calls these at fixed points of its own sequence, on the input tick and
//! on every rendered frame. Their owners live above the head pose, so they hand it their tables at
//! install ([`register_head_participants`] and [`register_body_frame_filter`]), before any hook
//! can tick the simulation or start a frame.

use std::sync::OnceLock;

use glam::{Quat, Vec3};
use jc3vr_core::published::Stamp;

use crate::headpose::{config::HeadPoseConfig, xr::FollowSettings};

/// The character state the head pose reads through its owners.
pub struct HeadParticipants {
    /// How the body follows the head this tick.
    pub follow_settings: fn() -> FollowSettings,
    /// When the on-foot orientation evaluator last ran, once per tick on foot; `None` before its
    /// first run. It is compared between input ticks.
    pub on_foot_ran: fn() -> Option<Stamp>,
    /// When a swim movement core last ran, compared the same way.
    pub swim_ran: fn() -> Option<Stamp>,
    /// Whether the local player swims.
    pub swimming: fn() -> bool,
    /// Whether a seat transition is entering or leaving a vehicle.
    pub vehicle_transition: fn() -> bool,
    /// Whether the body is thrown: ragdolling, getting up, or dead.
    pub body_thrown: fn() -> bool,
    /// The heading relative to the view the body follows while the boost wingsuit takes off from
    /// the ground, owned by the take-off; `None` outside it. Read on every input tick under VR.
    pub take_off_heading: fn() -> Option<f32>,
    /// Take the arm strokes' turn of the swimming body queued since the last call, radians to the
    /// right, owned by the strokes. Taken on every input tick under VR, in or out of the water.
    pub take_swim_turn: fn() -> f32,
    /// Whether the controllers' mode is the UI rather than gameplay.
    pub ui_mode: fn() -> bool,
}

/// A filter on the body frame and the anchor the head composes onto, which a feature that turns
/// or moves the character under the player registers so the view does not follow that motion.
/// Without one, the body frame and the anchor pass through unchanged.
pub struct BodyFrameFilter {
    /// Advance the filter one step: the body rotation the view composes with, the view head's
    /// offset from the character's root (`None` while the pose is frozen or without anchors),
    /// whether the head follows the character on foot, and the head pose settings. Runs on the
    /// input tick, with the simulation's lock held, and on every rendered frame.
    pub advance: fn(body: Quat, rooted: Option<Vec3>, on_foot: bool, config: &HeadPoseConfig),
    /// Apply the filter to a body rotation.
    pub filter_rotation: fn(body: Quat) -> Quat,
    /// Apply the filter to the head anchor.
    pub filter_anchor: fn(anchor: Vec3) -> Vec3,
    /// Whether the filter acts on the body frame at all, so the turn under the view is not
    /// folded out of a frame the filter replaces.
    pub engaged: fn() -> bool,
    /// Take the heading the filter wants the body turned toward, if it still wants it.
    pub take_body_yaw_retarget: fn() -> Option<f32>,
    /// Observe one input tick's advance: the body rotation it filtered and the tick's delta.
    pub observe_tick: fn(body: Quat, dt: f32),
    /// Observe one rendered frame's pose composition.
    pub observe_frame: fn(frame: &FrameTelemetry),
}

/// One rendered VR frame's pose composition, for the body frame filter's observer: the HMD cockpit
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

/// Hand the head pose its body frame filter. Called once at install; a second filter is ignored.
pub fn register_body_frame_filter(filter: &'static BodyFrameFilter) {
    if FILTER.set(filter).is_err() {
        tracing::warn!(
            "head participants: a body frame filter is already registered; the new one is ignored"
        );
    }
}

/// Whether a body frame filter is registered, for the install's registration check.
pub fn body_frame_filter_registered() -> bool {
    FILTER.get().is_some()
}

/// The registered participants. The simulation ticks and the frames run from hooks, and the table
/// is registered before the hooks are installed, so it is always present by then.
pub(crate) fn head_participants() -> &'static HeadParticipants {
    PARTICIPANTS
        .get()
        .expect("head participants: the table is registered before the hooks are installed")
}

/// The registered body frame filter, or the pass-through one when none is registered, as in a test
/// that drives the head composition without the install.
pub(crate) fn body_frame_filter() -> &'static BodyFrameFilter {
    FILTER.get().copied().unwrap_or(&PASS_THROUGH)
}

static PARTICIPANTS: OnceLock<&'static HeadParticipants> = OnceLock::new();

static FILTER: OnceLock<&'static BodyFrameFilter> = OnceLock::new();

static PASS_THROUGH: BodyFrameFilter = BodyFrameFilter {
    advance: |_, _, _, _| {},
    filter_rotation: |body| body,
    filter_anchor: |anchor| anchor,
    engaged: || false,
    take_body_yaw_retarget: || None,
    observe_tick: |_, _| {},
    observe_frame: |_| {},
};
