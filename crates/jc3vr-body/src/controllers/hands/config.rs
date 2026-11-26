//! Controller-owned hands' settings and their wrist-to-grip offset.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

use crate::controllers::{hands::anchor::AnchorConfig, presentation::PresentationConfig};

/// The controller-owned hands' settings, at `controllers.hands` in the settings file; the payload
/// binds the slot at startup.
pub static HANDS_CONFIG: ConfigSlot<HandsConfig> = ConfigSlot::new("controllers.hands");

/// Whether a mounted gun's grips anchor the hands, at `controllers.vehicles.mounted.aim_by_hands`
/// in the settings file: a field of the vehicles' group, which the hand tiers read to anchor the
/// hands on a mounted gun's grips. The payload binds the slot at startup.
pub static MOUNTED_AIM_BY_HANDS: ConfigSlot<bool> =
    ConfigSlot::new("controllers.vehicles.mounted.aim_by_hands");

/// Controller-owned hands (see [`crate::controllers::hands`]). Every part acts only under the
/// controller scheme and only for a hand whose controller is tracked, so the parts default on.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HandsConfig {
    /// Drive the arms with HumanIK from the shoulders to the controller poses, with the wrists
    /// taking the controller rotations.
    pub ik: bool,
    /// Re-pose the hand and forearm bones and the held weapon at render time from the latest
    /// controller pose, removing the sim tick's lag behind the controllers.
    pub late_update: bool,
    /// Re-pose the upper body (the spine, the head, and the arms) at render time from the latest
    /// tracking, on top of the late update's hands.
    pub presentation: PresentationConfig,
    /// Hold a gun in a controller-driven hand fixed to the wrist at the hand's bind-pose grip,
    /// rather than where the animation puts the attach bone.
    pub rigid_grip: bool,
    /// Where the character's wrist sits relative to the controller's grip pose.
    pub grip: GripOffset,
    /// The anchored tier of the grapple states, and the hang movement it drives.
    pub anchor: AnchorConfig,
}

impl HandsConfig {
    pub const fn new() -> Self {
        Self {
            ik: true,
            late_update: true,
            presentation: PresentationConfig::new(),
            rigid_grip: true,
            grip: GripOffset::INDEX_BEST_GUESS,
            anchor: AnchorConfig::new(),
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
    /// A best guess for the Valve Index controller, not yet fitted in the headset: the wrist a few
    /// centimetres behind and to the back-of-hand side of the grip centre, and the handle tilted
    /// toward the fingers as a power grip holds it.
    pub const INDEX_BEST_GUESS: GripOffset = GripOffset {
        position_m: glam::Vec3::new(0.02, 0.075, 0.0),
        rotation_deg: glam::Vec3::new(15.0, 0.0, 0.0),
    };
}
