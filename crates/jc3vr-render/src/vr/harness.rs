//! What a capture harness above the VR runtime imposes on the frame: whether it holds a visual
//! configuration, and the headset-like rig it substitutes for the located views.
//!
//! The harness lives above the runtime, so it pushes both into this module as they change, and the
//! frame loop, the per-eye parameters, and the camera hooks read them here. With no harness
//! configuration in force, neither is set and the frame runs as located.

use std::sync::atomic::{AtomicBool, Ordering};

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use crate::vr::Fov;

/// A headset-like rig substituted for the located views: the distance between the eyes and each
/// eye's view relative to the head.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadsetSubstitute {
    /// The distance between the eyes, in metres, along the head's horizontal axis.
    pub ipd_m: f32,
    /// The left eye, then the right.
    pub eyes: [EyeOffset; 2],
}

/// One eye's view relative to the head, as a [`HeadsetSubstitute`] holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EyeOffset {
    /// The eye's turn relative to the head.
    pub orientation: Quat,
    /// The eye's field of view.
    pub fov: Fov,
}

/// One eye's substituted view: its pose in the frame the located views are in and its field of
/// view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EyeSubstitute {
    pub position: Vec3,
    pub orientation: Quat,
    pub fov: Fov,
}

impl HeadsetSubstitute {
    /// Each eye's substituted view for a head at `head_position` with `head_orientation`, in the
    /// frame the located views are in.
    pub fn substitute(&self, head_position: Vec3, head_orientation: Quat) -> [EyeSubstitute; 2] {
        std::array::from_fn(|eye| {
            let offset = self.eyes[eye];
            let side = if eye == 0 { -0.5 } else { 0.5 };
            EyeSubstitute {
                position: head_position + head_orientation * Vec3::new(side * self.ipd_m, 0.0, 0.0),
                orientation: (head_orientation * offset.orientation).normalize(),
                fov: offset.fov,
            }
        })
    }
}

/// Record whether the visual harness holds a configuration. The harness sets it as its mode
/// changes.
pub fn set_visual_active(active: bool) {
    VISUAL_ACTIVE.store(active, Ordering::Relaxed);
}

/// Whether the visual harness holds a configuration.
pub(crate) fn visual_active() -> bool {
    VISUAL_ACTIVE.load(Ordering::Relaxed)
}

/// Set the rig the frame loop substitutes for the located views, or `None` to leave them as
/// located.
pub fn set_headset_substitute(substitute: Option<HeadsetSubstitute>) {
    *HEADSET.lock() = substitute;
}

/// The rig the frame loop substitutes for the located views, or `None` to leave them as located.
pub(crate) fn headset_substitute() -> Option<HeadsetSubstitute> {
    *HEADSET.lock()
}

static VISUAL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// The rig in force. A lock of its own, never held while taking another, because the frame loop
/// reads it under the runtime lock.
static HEADSET: Mutex<Option<HeadsetSubstitute>> = Mutex::new(None);
