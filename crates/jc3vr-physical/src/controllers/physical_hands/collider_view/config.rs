//! The collider view's settings.

use serde::{Deserialize, Serialize};

/// The collider view: the physics world's bodies near the player drawn in the world-space overlay.
/// A diagnostic, off by default.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColliderViewConfig {
    /// Draw the view.
    pub enabled: bool,
    /// Draw each body's box, as the broad phase holds it.
    pub boxes: bool,
    /// Draw each body's shape where it is a sphere, a capsule, or a convex hull.
    pub shapes: bool,
    /// Draw static bodies: the terrain and the buildings, whose boxes can be large.
    pub static_bodies: bool,
    /// Flash the contacts the hand bodies and the held guns' bodies start, and the player's
    /// ground.
    pub contacts: bool,
    /// Draw the hands' grip joints and the held guns' joints.
    pub joints: bool,
    /// Pick the body under the gaze for the debug UI's readout, and mark it.
    pub gaze: bool,
}

impl ColliderViewConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            boxes: true,
            shapes: true,
            static_bodies: true,
            contacts: true,
            joints: true,
            gaze: true,
        }
    }
}

impl Default for ColliderViewConfig {
    fn default() -> Self {
        Self::new()
    }
}
