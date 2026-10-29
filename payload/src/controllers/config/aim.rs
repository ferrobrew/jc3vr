//! Controller-owned hands, their wrist-to-grip offset, and per-hand aiming.

use serde::{Deserialize, Serialize};

use crate::controllers::{
    aim::sights::config::{SightTable, SightTuningConfig},
    hands::anchor::AnchorConfig,
    presentation::PresentationConfig,
};

/// The aim-state work (see [`crate::controllers::aim_state`]): the pins, the neutral aim point, the
/// wall-block clear, and the mental-state cuts act under the controller scheme whenever it is in
/// effect; the arm aim IK skip has a switch.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AimStateConfig {
    /// Skip the local player's arm and upper-body aim IK tasks, because the controllers own the
    /// arms.
    pub skip_arm_ik: bool,
}

impl AimStateConfig {
    pub const fn new() -> Self {
        Self { skip_arm_ik: true }
    }
}

impl Default for AimStateConfig {
    fn default() -> Self {
        Self::new()
    }
}

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

/// Per-hand aiming (see [`crate::controllers::aim`]). Acts only under the controller scheme. The
/// rays' fixed reach is [`RAY_LENGTH_M`](crate::controllers::aim::RAY_LENGTH_M).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AimConfig {
    /// Aim the held guns along the dominant hand's sight line (and, dual-wielding, each pistol
    /// along its own hand's).
    pub gun: bool,
    /// Aim the grapple along the off-hand controller's ray.
    pub grapple: bool,
    /// How strongly the written aim point is pulled toward the best candidate target along each
    /// hand ray, `0..=1`. Zero disables aim assist.
    pub assist_strength: f32,
    /// Per-weapon corrections to the sight line derived from the fire point. An entry here takes
    /// precedence over the embedded sight table
    /// ([`crate::controllers::aim::sights`]); a weapon in neither aims from the fire point.
    pub sights: SightTable,
    /// The in-headset sight tuning tool.
    pub sight_tuning: SightTuningConfig,
}

impl AimConfig {
    pub const fn new() -> Self {
        Self {
            gun: true,
            grapple: true,
            assist_strength: 0.0,
            sights: SightTable::EMPTY,
            sight_tuning: SightTuningConfig::new(),
        }
    }
}

impl Default for AimConfig {
    fn default() -> Self {
        Self::new()
    }
}
