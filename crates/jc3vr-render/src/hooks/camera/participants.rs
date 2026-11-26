//! What the layers above the camera hooks contribute to them: the sim-phase camera matrix the
//! grapple's scoped aim calls read in place of the camera's own, the observer of each gameplay
//! frame's camera placement, and the observer of the render camera the floating panel and the
//! pointer share.
//!
//! Each is registered once at startup by the module that owns it, so the camera hooks name none of
//! them. Until one is registered the hooks behave as if it had nothing to say: no override, and
//! nobody observing.

use std::sync::OnceLock;

use glam::{Mat4, Vec3};
use jc3gi::types::math::Matrix4;

use crate::headpose::xr::CockpitPose;

/// The camera's gameplay-side participants.
#[derive(Clone, Copy)]
pub(crate) struct CameraParticipants {
    /// The camera matrix the camera getter returns in place of the camera's own, when one stands in
    /// for the camera on the calling thread.
    pub(crate) matrix_override: fn() -> Option<Matrix4>,
    /// Observes a gameplay frame's camera placement: the character world matrix the placement
    /// composed with, and the camera's eye point.
    pub(crate) placed: fn(Mat4, Vec3),
}

/// An observer of the un-offset centre render camera's world transform, with the cockpit head pose
/// and the world scale that placed it.
pub(crate) type RenderCameraObserver = fn(Mat4, Option<CockpitPose>, f32);

/// Register the camera's gameplay-side participants. The first registration stands.
pub(crate) fn register_camera_participants(participants: CameraParticipants) {
    if PARTICIPANTS.set(participants).is_err() {
        tracing::warn!("camera: participants are already registered; the new ones are ignored");
    }
}

/// Register the render camera's observer. The first registration stands.
pub(crate) fn register_render_camera_observer(observer: RenderCameraObserver) {
    if RENDER_CAMERA_OBSERVER.set(observer).is_err() {
        tracing::warn!(
            "camera: a render camera observer is already registered; the new one is ignored"
        );
    }
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

pub(super) fn publish_render_camera(world: Mat4, cockpit: Option<CockpitPose>, scale: f32) {
    if let Some(observer) = RENDER_CAMERA_OBSERVER.get() {
        observer(world, cockpit, scale);
    }
}

static PARTICIPANTS: OnceLock<CameraParticipants> = OnceLock::new();

static RENDER_CAMERA_OBSERVER: OnceLock<RenderCameraObserver> = OnceLock::new();
