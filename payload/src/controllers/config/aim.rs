//! Controller-owned hands, their wrist-to-grip offset, and per-hand aiming.

use serde::{Deserialize, Serialize};

use crate::controllers::{
    aim::sights::config::{SightTable, SightTuningConfig},
    hands::{anchor::AnchorConfig, reach::ReachConfig, tiers::HandTierTable},
};

/// The aim-state pin and the mental-state cuts (see [`crate::controllers::aim_state`]). Each part
/// has its own toggle so that it can be tested alone, and all of them act only under the
/// controller scheme unless [`without_controllers`](Self::without_controllers) is set.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AimStateConfig {
    /// Pin the local player's weapon aim clear, so that the body stays in the no-aim animation
    /// family while firing still works.
    pub pin: bool,
    /// Also pin the grapple aim clear, which the weapon pin does not reach. It changes how the
    /// body behaves on a held tether: the body no longer turns to face the hook.
    pub pin_grapple: bool,
    /// Skip the local player's arm and upper-body aim IK tasks, because the controllers own the
    /// arms.
    pub skip_arm_ik: bool,
    /// Hold the body's aim point neutral, ahead of the body, so that nothing turns or twists the
    /// body toward where the gun points. The weapons keep aiming where the game aims them.
    pub neutral_aim_point: bool,
    /// How far ahead of the head the neutral aim point sits, metres.
    pub neutral_aim_distance_m: f32,
    /// Clear the wall-penetration block on the player's held weapon, which the game computes from
    /// where the animation would put the barrel rather than where the hand holds it.
    pub clear_wall_block: bool,
    /// Remove the combat mental state's effects on the player's body: no automatic draw when
    /// combat starts and no automatic holster when it ends. The mental state itself keeps running
    /// for AI perception, and a deliberate holster still works.
    pub mental_state_cuts: bool,
    /// Apply every enabled part above under the gamepad scheme too, for testing the pin without
    /// controllers. Off by default: it changes how the body behaves for every player.
    pub without_controllers: bool,
}

impl AimStateConfig {
    pub const fn new() -> Self {
        Self {
            pin: true,
            pin_grapple: true,
            skip_arm_ik: true,
            neutral_aim_point: true,
            neutral_aim_distance_m: 25.0,
            clear_wall_block: true,
            mental_state_cuts: true,
            without_controllers: false,
        }
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
    /// Switch off the game's secondary hand IK (the weapon's off-hand grip, the grapple fire arm,
    /// and the parachute handles) for every hand the controllers drive.
    pub suppress_secondary_ik: bool,
    /// Hold a gun in a controller-driven hand fixed to the wrist at the hand's bind-pose grip,
    /// rather than where the animation puts the attach bone.
    pub rigid_grip: bool,
    /// The wrist targets' positional reach, `0..=1`, before the tier weight scales it.
    pub reach_t: f32,
    /// The wrist targets' rotational reach, `0..=1`, before the tier weight scales it.
    pub reach_r: f32,
    /// How far a wrist target out of reach pulls the torso after it, `0..=1`.
    pub pull: f32,
    /// How strongly a wrist resists being dragged by other effectors' pull, `0..=1`.
    pub resist: f32,
    /// Where the character's wrist sits relative to the controller's grip pose.
    pub grip: GripOffset,
    /// How long a hand takes to blend from its controller to the animation when a state yields
    /// it, seconds.
    pub yield_blend_s: f32,
    /// How long a hand takes to blend from the animation back to its controller, seconds.
    pub return_blend_s: f32,
    /// The hand tier of each scripted state.
    pub tiers: HandTierTable,
    /// The anchored tier of the grapple states, and the hang movement it drives.
    pub anchor: AnchorConfig,
    /// How the character's arm reaches for the controllers: the remap about his measured shoulders,
    /// the pull near full extension, and the render-time detach.
    pub reach: ReachConfig,
}

impl HandsConfig {
    pub const fn new() -> Self {
        Self {
            ik: true,
            late_update: true,
            suppress_secondary_ik: true,
            rigid_grip: true,
            reach_t: 1.0,
            reach_r: 1.0,
            pull: 0.0,
            resist: 0.0,
            grip: GripOffset::INDEX_BEST_GUESS,
            yield_blend_s: 0.25,
            return_blend_s: 0.35,
            tiers: HandTierTable::PLAN,
            anchor: AnchorConfig::new(),
            reach: ReachConfig::new(),
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

/// Per-hand aiming (see [`crate::controllers::aim`]). Acts only under the controller scheme.
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
    /// The farthest a candidate may be from the hand for aim assist, metres.
    pub assist_max_distance_m: f32,
    /// How far short of an aim-assist candidate's body position the line of sight to it may be
    /// blocked and still count as clear, metres: the line ends inside the target's own body.
    pub assist_occlusion_tolerance_m: f32,
    /// How far the hand rays are cast, metres. A gun ray that hits nothing aims at this distance
    /// along the ray.
    pub max_distance_m: f32,
    /// The nearest a gun's aim point may be to the sight's origin, metres. A nearer hit is pushed
    /// out along the ray, because the game fires along the barrel instead of at an aim point near
    /// the muzzle, and skips the weapon's update entirely for one almost at the muzzle.
    pub min_target_distance_m: f32,
    /// Where the grapple ray starts relative to the off-hand's wrist, in the wrist frame (-Z toward
    /// the fingers, +Y out of the back of the hand), metres. The gauntlet sits on the forearm just
    /// behind the wrist.
    pub grapple_origin_m: glam::Vec3,
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
            assist_max_distance_m: 150.0,
            assist_occlusion_tolerance_m: 0.75,
            max_distance_m: 500.0,
            min_target_distance_m: 2.0,
            grapple_origin_m: glam::Vec3::ZERO,
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
