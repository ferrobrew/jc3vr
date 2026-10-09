//! The collider view's settings.

use serde::{Deserialize, Serialize};

/// The collider view: the physics world's bodies near the player drawn in the world-space overlay.
/// A diagnostic, off by default.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColliderViewConfig {
    /// Draw the view.
    pub enabled: bool,
    /// How far from the player a body's box may be and still be drawn, metres.
    pub radius_m: f32,
    /// The most bodies drawn, the nearest first.
    pub max_bodies: u32,
    /// The most overlay primitives the view submits in a frame. The overlay's own cap covers every
    /// readout together, and the view's slot comes last, so the view gives way first.
    pub max_primitives: u32,
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
            radius_m: 6.0,
            max_bodies: 128,
            max_primitives: 1536,
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
