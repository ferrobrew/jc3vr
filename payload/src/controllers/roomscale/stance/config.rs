//! The standing stance's settings, under `controllers.roomscale.stance`.

use serde::{Deserialize, Serialize};

/// Squaring the character's standing stance with the player's. Both parts act only under the
/// controller scheme with the headset driving the view, on foot, while the character stands still in
/// the base idle state and is not aiming, holding a grapple tether, or crouched.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StanceConfig {
    /// Stage one: while standing still, keep the wielded weapon's animation modifiers off the body's
    /// rule system, so the idle plays its unarmed relaxed clip instead of the weapon's held stance.
    /// The upper-body graphs keep the modifiers. Off by default: the relaxed clip's arms and hand
    /// pose take no account of the weapon, which the controller hands cover only while tracked.
    pub relaxed_idle: bool,
    /// Stage two: while standing still, square the stance with HumanIK: the ankles placed side by
    /// side across the body's forward, the pelvis and chest turned to face it, and the feet turned
    /// to point along it. Off by default: it admits the legs to the body solve on every standing
    /// tick, and it has not been checked in the headset.
    pub square: bool,
    /// How long the character must have stood still before the squaring starts, seconds, so that a
    /// stop's settling animation is not fought.
    pub settle_s: f32,
    /// How long the squaring takes to blend in, seconds.
    pub blend_in_s: f32,
    /// How long the squaring takes to blend out when the character starts moving or another
    /// condition ends it, seconds.
    pub blend_out_s: f32,
    /// The horizontal speed below which the character counts as standing still, metres per second.
    /// The roomscale chase moves the body without leaving the idle state.
    pub still_speed_mps: f32,
    /// The distance between the ankles across the body, metres at the character's scale. Zero keeps
    /// the animated spacing and removes only the stagger.
    pub feet_width_m: f32,
    /// How much of the pelvis's turn away from the body's forward is removed, `0..=1`.
    pub pelvis_share: f32,
    /// How much of the chest's turn away from the body's forward is removed, `0..=1`. Carried by
    /// the body IK's torso rotation, so it needs the body IK on.
    pub chest_share: f32,
    /// How much of each foot's turn away from the body's forward is removed, `0..=1`.
    pub feet_yaw_share: f32,
    /// The resistance written for the ankles while their rotation is held, `0..=1`, so that a reach
    /// elsewhere in the body does not drag the planted feet.
    pub ankle_resist: f32,
}

impl StanceConfig {
    pub const fn new() -> Self {
        Self {
            relaxed_idle: false,
            square: false,
            settle_s: 0.4,
            blend_in_s: 0.5,
            blend_out_s: 0.15,
            still_speed_mps: 0.15,
            feet_width_m: 0.28,
            pelvis_share: 1.0,
            chest_share: 1.0,
            feet_yaw_share: 0.8,
            ankle_resist: 0.8,
        }
    }
}

impl Default for StanceConfig {
    fn default() -> Self {
        Self::new()
    }
}
