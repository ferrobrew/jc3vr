//! The hands' tuning, read from the shipped `tuning/hands.toml` ([`HANDS`]): the blends between the
//! controller and the animation, the grapple hand's anchor and hang swing, the palm's centre, the
//! arm reach and the rendered hand's detach, the wrist targets' reaches, and the rigid grip's
//! automatic choice.

use glam::Vec3;
use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range, vec3},
    tuning::Tuning,
};
use serde::Deserialize;

pub use crate::controllers::hands::reach::ReachTuning;

jc3vr_core::tuning_file! {
    /// The hands' tuning file.
    pub static HANDS: HandsTuning = "hands.toml";
}

/// The hands' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandsTuning {
    pub tiers: TiersTuning,
    pub anchor: AnchorTuning,
    pub pose: HandPoseTuning,
    pub reach: ReachTuning,
    pub ik: WristTargetTuning,
    pub grip: RigidGripTuning,
}

impl Tuning for HandsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let t = &self.tiers;
        positive("tiers.yield_blend_s", t.yield_blend_s, 5.0)?;
        positive("tiers.return_blend_s", t.return_blend_s, 5.0)?;
        let a = &self.anchor;
        positive("anchor.blend_s", a.blend_s, 5.0)?;
        nonnegative("anchor.swing_deadzone_m", a.swing_deadzone_m, 1.0)?;
        range(
            "anchor.swing_full_m",
            a.swing_full_m,
            a.swing_deadzone_m + f32::EPSILON,
            2.0,
        )?;
        nonnegative("anchor.swing_scale", a.swing_scale, 10.0)?;
        vec3("pose.palm_centre_m", self.pose.palm_centre_m, 0.2)?;
        self.reach.validate()?;
        let ik = &self.ik;
        range("ik.secondary_hold_reach", ik.secondary_hold_reach, 0.0, 1.0)?;
        range("ik.wrist_reach_t", ik.wrist_reach_t, 0.0, 1.0)?;
        range("ik.wrist_reach_r", ik.wrist_reach_r, 0.0, 1.0)?;
        range("ik.wrist_pull", ik.wrist_pull, 0.0, 1.0)?;
        range("ik.wrist_resist", ik.wrist_resist, 0.0, 1.0)?;
        range("grip.auto_turn_deg", self.grip.auto_turn_deg, 0.0, 180.0)
    }
}

/// The per-hand blend between the controller and the animation
/// ([`tiers`](crate::controllers::hands::tiers)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TiersTuning {
    /// How long a hand takes to blend from its controller to the animation when a state yields it,
    /// seconds.
    pub(crate) yield_blend_s: f32,
    /// How long a hand takes to blend from the animation back to its controller, seconds.
    pub(crate) return_blend_s: f32,
}

/// The anchored tier of the grapple states ([`anchor`](crate::controllers::hands::anchor)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorTuning {
    /// How long the hand takes to turn onto the line and back off it, seconds. The vehicle
    /// controls' and handles' anchored poses blend over it too.
    pub blend_s: f32,
    /// The hand displacement below which the hang is left alone, metres.
    pub(crate) swing_deadzone_m: f32,
    /// The hand displacement that swings at full strength, metres.
    pub(crate) swing_full_m: f32,
    /// The full-strength swing impulse per hang tick, as a multiple of the game's own continuous
    /// stick swing at full deflection.
    pub(crate) swing_scale: f32,
}

/// The hand's own geometry ([`pose`](crate::controllers::hands::pose)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandPoseTuning {
    /// The centre of the palm in the wrist frame, metres of the player's hand: a little toward the
    /// fingers and into the palm from the wrist joint.
    pub palm_centre_m: Vec3,
}

/// The wrist targets queued on HumanIK ([`ik`](crate::controllers::hands::ik)), each `0..=1`
/// before the tier weight scales it.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WristTargetTuning {
    /// The reach of the weak `SECONDARY` target that holds a driven wrist while the weapon off-hand
    /// grip solves for the other.
    pub(crate) secondary_hold_reach: f32,
    /// The wrist targets' positional reach.
    pub(crate) wrist_reach_t: f32,
    /// The wrist targets' rotational reach.
    pub(crate) wrist_reach_r: f32,
    /// How far a wrist target out of reach pulls the torso after it, before the reach ramp raises
    /// it near full extension.
    pub(crate) wrist_pull: f32,
    /// How strongly a wrist resists being dragged by other effectors' pull.
    pub(crate) wrist_resist: f32,
}

/// The rigid grip ([`grip`](crate::controllers::hands::grip)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigidGripTuning {
    /// How far the right hand's own grip may turn from the mirrored one before the automatic choice
    /// takes the mirrored one, degrees.
    pub(crate) auto_turn_deg: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(HANDS.get().validate(), Ok(()));
    }
}
