//! Controller-owned hands' wrist-to-grip offset.

use jc3vr_core::config::ConfigSlot;
use serde::{Deserialize, Serialize};

/// The controller-owned hands' settings, at `controllers.hands` in the settings file; the payload
/// binds the slot at startup.
pub static HANDS_CONFIG: ConfigSlot<HandsConfig> = ConfigSlot::new("controllers.hands");

/// Controller-owned hands (see [`crate::controllers::hands`]): their calibration. Every part of the
/// hands acts only under the controller scheme and only for a hand whose controller is tracked.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HandsConfig {
    /// Where the character's wrist sits relative to the controller's grip pose.
    pub grip: GripOffset,
}

impl HandsConfig {
    pub const fn new() -> Self {
        Self {
            grip: GripOffset::INDEX_BEST_GUESS,
        }
    }
}

impl Default for HandsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The offset from a controller's OpenXR grip pose to the character's wrist, given for the right
/// hand in the grip frame and mirrored for the left (see
/// [`grip_to_wrist`](crate::controllers::hands::pose::grip_to_wrist)).
///
/// The wrist frame is the hand-tracking wrist convention: the origin at the wrist joint, -Z toward
/// the fingers, and +Y out of the back of the hand. With no adjustment the fingers point along the
/// grip's -Y, square to the handle, with the back of the right hand along the grip's +X; the
/// adjustment rotates the hand about the grip axes. The grip frame is OpenXR's: -Z up the handle
/// toward the thumb, +X into the right palm, and +Y back toward the wrist.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GripOffset {
    /// The wrist joint's position in the right grip frame, metres.
    pub position_m: glam::Vec3,
    /// The hand's rotation in the right grip frame, applied about the grip's X, then Y, then Z
    /// axes, degrees.
    pub rotation_deg: glam::Vec3,
}

impl GripOffset {
    /// A best guess for the Valve Index controller, not fitted in the headset: the wrist a few
    /// centimetres behind and to the back-of-hand side of the grip centre, and the handle tilted
    /// toward the fingers as a power grip holds it.
    pub const INDEX_BEST_GUESS: GripOffset = GripOffset {
        position_m: glam::Vec3::new(0.02, 0.075, 0.0),
        rotation_deg: glam::Vec3::new(15.0, 0.0, 0.0),
    };
}
