//! What the layers above the camera hooks contribute to them: the sim-phase camera matrix the
//! grapple's scoped aim calls read in place of the camera's own, the observer of each gameplay
//! frame's camera placement, and the debug tools' taps on the scene render camera's setup.
//!
//! Each is registered once at startup by the module that owns it, so the camera hooks name none of
//! them. Until one is registered the hooks behave as if it had nothing to say: no override, and
//! nobody observing.

use std::sync::OnceLock;

use glam::{Mat4, Vec3};
use jc3gi::types::math::Matrix4;

/// The camera's gameplay-side participants.
#[derive(Clone, Copy)]
pub struct CameraParticipants {
    /// The camera matrix the camera getter returns in place of the camera's own, when one stands in
    /// for the camera on the calling thread.
    pub matrix_override: fn() -> Option<Matrix4>,
    /// Observes a gameplay frame's camera placement: the character world matrix the placement
    /// composed with, and the camera's eye point.
    pub placed: fn(Mat4, Vec3),
}

/// The debug tools' tap on `SetupRenderCamera` for the scene render camera. It fires on entry,
/// before the hook publishes the camera's world transform.
#[derive(Clone, Copy)]
pub struct CameraDebugTaps {
    /// The hook was entered for the scene render camera, before anything in it ran.
    pub setup_entered: fn(),
}

/// Register the camera's gameplay-side participants. The first registration stands.
pub fn register_camera_participants(participants: CameraParticipants) {
    if PARTICIPANTS.set(participants).is_err() {
        tracing::warn!("camera: participants are already registered; the new ones are ignored");
    }
}

/// Register the debug tools' taps on the render camera's setup. The first registration stands.
pub fn register_camera_debug_taps(taps: CameraDebugTaps) {
    if DEBUG_TAPS.set(taps).is_err() {
        tracing::warn!("camera: debug taps are already registered; the new ones are ignored");
    }
}

/// Whether the camera's gameplay-side participants are registered.
pub fn camera_participants_registered() -> bool {
    PARTICIPANTS.get().is_some()
}

/// Whether the debug tools' taps are registered.
pub fn camera_debug_taps_registered() -> bool {
    DEBUG_TAPS.get().is_some()
}

pub(super) fn matrix_override() -> Option<Matrix4> {
    PARTICIPANTS
        .get()
        .and_then(|participants| (participants.matrix_override)())
}

pub(super) fn note_placement(character: Mat4, camera: Vec3) {
    if let Some(participants) = PARTICIPANTS.get() {
        (participants.placed)(character, camera);
    }
}

pub(super) fn tap_setup_entered() {
    if let Some(taps) = DEBUG_TAPS.get() {
        (taps.setup_entered)();
    }
}

static PARTICIPANTS: OnceLock<CameraParticipants> = OnceLock::new();

static DEBUG_TAPS: OnceLock<CameraDebugTaps> = OnceLock::new();
