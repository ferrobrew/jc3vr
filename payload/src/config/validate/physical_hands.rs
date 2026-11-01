//! Checks for physical hand handling: the throw, the hands' strength, the brace's rebuild and break, and
//! climbing.

use crate::{
    config::validate::{Invalid, nonnegative, range},
    controllers::physical_hands::PhysicalHandsConfig,
};

/// Check the physical hand settings. Paths are relative to `controllers.physical_hands`.
pub(super) fn physical_hands(p: &PhysicalHandsConfig) -> Result<(), Invalid> {
    nonnegative(
        "physical_hands.pickup.throw_scale",
        p.pickup.throw_scale,
        20.0,
    )?;
    range(
        "physical_hands.bodies.strength",
        p.bodies.strength,
        0.0,
        10.0,
    )?;
    nonnegative(
        "physical_hands.bodies.brace.distance_m",
        p.bodies.brace.distance_m,
        1.0,
    )?;
    range(
        "physical_hands.bodies.brace.angle_rad",
        p.bodies.brace.angle_rad,
        0.0,
        std::f32::consts::PI,
    )?;
    nonnegative(
        "physical_hands.bodies.brace.min_interval_s",
        p.bodies.brace.min_interval_s,
        5.0,
    )?;
    nonnegative(
        "physical_hands.bodies.brace.break_distance_m",
        p.bodies.brace.break_distance_m,
        1.0,
    )?;
    nonnegative(
        "physical_hands.climb.fling_scale",
        p.climb.fling_scale,
        10.0,
    )
}
