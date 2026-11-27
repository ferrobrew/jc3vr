//! The aim's tuning, read from the shipped `tuning/aim.toml` ([`AIM`]): how far the hand rays reach
//! and where the grapple's starts, the aim assist's reach and cone, the grapple's weapon cast, the
//! world markers' raycast budget, the neutral aim point, and the sight tuning tool's marker. The
//! player's aim settings stay in [`AimConfig`](crate::controllers::aim::config::AimConfig).

use glam::Vec3;
use jc3vr_core::{
    config::{Invalid, positive, range, vec3},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The aim's tuning file.
    pub static AIM: AimTuning = "aim.toml";
}

/// The aim's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AimTuning {
    pub ray: RayTuning,
    pub assist: AssistTuning,
    pub grapple_weapons: GrappleWeaponsTuning,
    pub marker_rays: MarkerRaysTuning,
    pub neutral: NeutralAimTuning,
    pub(crate) sight_tool: SightToolTuning,
}

impl Tuning for AimTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("ray.length_m", self.ray.length_m, 10_000.0)?;
        vec3("ray.grapple_origin_m", self.ray.grapple_origin_m, 0.5)?;
        range(
            "ray.min_target_distance_m",
            self.ray.min_target_distance_m,
            0.0,
            self.ray.length_m,
        )?;
        positive(
            "assist.max_distance_m",
            self.assist.max_distance_m,
            10_000.0,
        )?;
        positive(
            "assist.grapple_cone_deg",
            self.assist.grapple_cone_deg,
            90.0,
        )?;
        range(
            "assist.body_sight_tolerance_m",
            self.assist.body_sight_tolerance_m,
            0.0,
            10.0,
        )?;
        if !(1..=16).contains(&self.grapple_weapons.max_steps) {
            return Err(Invalid(format!(
                "grapple_weapons.max_steps must be in [1, 16], got {}",
                self.grapple_weapons.max_steps
            )));
        }
        positive(
            "grapple_weapons.step_past_m",
            self.grapple_weapons.step_past_m,
            1.0,
        )?;
        if !(1..=256).contains(&self.marker_rays.max_casts_per_tick) {
            return Err(Invalid(format!(
                "marker_rays.max_casts_per_tick must be in [1, 256], got {}",
                self.marker_rays.max_casts_per_tick
            )));
        }
        positive("neutral.distance_m", self.neutral.distance_m, 1_000.0)?;
        positive(
            "sight_tool.marker_distance_m",
            self.sight_tool.marker_distance_m,
            100.0,
        )
    }
}

/// The hand rays.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RayTuning {
    /// How far the hand rays are cast, metres. A gun ray that hits nothing aims at this distance
    /// along the ray.
    pub length_m: f32,
    /// Where the grapple ray starts relative to the off-hand's wrist, in the wrist frame (-Z toward
    /// the fingers, +Y out of the back of the hand), metres: where the gauntlet's line leaves the
    /// forearm.
    pub grapple_origin_m: Vec3,
    /// The nearest a gun's aim point may be to the sight's origin, metres. A nearer hit is pushed
    /// out along the ray, because the game fires along the barrel instead of at an aim point near
    /// the muzzle, and skips the weapon's update entirely for one almost at the muzzle.
    pub(crate) min_target_distance_m: f32,
}

/// The aim assist.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistTuning {
    /// The farthest a candidate may be from the hand for aim assist, metres.
    pub max_distance_m: f32,
    /// The half-angle of the cone around the grapple hand's ray that the grapple's aim assist
    /// looks for a target in, degrees.
    pub(crate) grapple_cone_deg: f32,
    /// How far short of a target's body position the line of sight to it may be blocked and still
    /// count as clear, metres: the line ends inside the target's own body, or its vehicle's.
    pub(crate) body_sight_tolerance_m: f32,
}

/// The grapple's cast for the weapons lying in the world.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrappleWeaponsTuning {
    /// The most hits the weapon cast steps past (a character's capsule, a projectile, a vehicle's
    /// collision body) on its way to a weapon.
    pub max_steps: usize,
    /// How far past a hit the next step of the weapon cast starts, metres.
    pub(crate) step_past_m: f32,
}

/// The world markers' fallback raycasts.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerRaysTuning {
    /// The most rays cast per simulation update; the rest wait for a later update.
    pub(crate) max_casts_per_tick: usize,
}

/// The neutral aim point the body turns toward.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeutralAimTuning {
    /// How far ahead of the head the neutral aim point sits, metres.
    pub distance_m: f32,
}

/// The in-headset sight tuning tool.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SightToolTuning {
    /// How far ahead of the head the line-up marker is placed, metres.
    pub(crate) marker_distance_m: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(AIM.get().validate(), Ok(()));
    }
}
