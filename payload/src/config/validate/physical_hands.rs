//! Checks for physical hand handling: the throw, the hands' strength, and climbing.

use crate::{
    config::validate::{Invalid, nonnegative, positive, range},
    controllers::physical_hands::{PhysicalHandsConfig, climb::ClimbConfig},
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
    climb(&p.climb)
}

fn climb(c: &ClimbConfig) -> Result<(), Invalid> {
    nonnegative("physical_hands.climb.reach_m", c.reach_m, 1.0)?;
    range("physical_hands.climb.gain", c.gain, 0.0, 1.0)?;
    nonnegative("physical_hands.climb.max_speed_mps", c.max_speed_mps, 50.0)?;
    positive(
        "physical_hands.climb.break_distance_m",
        c.break_distance_m,
        10.0,
    )?;
    nonnegative("physical_hands.climb.fling_scale", c.fling_scale, 10.0)?;
    nonnegative(
        "physical_hands.climb.max_fling_speed_mps",
        c.max_fling_speed_mps,
        100.0,
    )?;
    nonnegative(
        "physical_hands.climb.fling_smoothing_s",
        c.fling_smoothing_s,
        10.0,
    )?;
    nonnegative("physical_hands.climb.mantle_start_m", c.mantle_start_m, 5.0)?;
    nonnegative(
        "physical_hands.climb.mantle_clearance_m",
        c.mantle_clearance_m,
        2.0,
    )?;
    nonnegative(
        "physical_hands.climb.mantle_forward_m",
        c.mantle_forward_m,
        5.0,
    )?;
    positive(
        "physical_hands.climb.mantle_speed_mps",
        c.mantle_speed_mps,
        20.0,
    )?;
    nonnegative(
        "physical_hands.climb.mantle_min_speed_mps",
        c.mantle_min_speed_mps,
        20.0,
    )?;
    nonnegative(
        "physical_hands.climb.mantle_min_climb_m",
        c.mantle_min_climb_m,
        5.0,
    )?;
    nonnegative(
        "physical_hands.climb.mantle_land_past_m",
        c.mantle_land_past_m,
        5.0,
    )?;
    range(
        "physical_hands.climb.min_ledge_normal_y",
        c.min_ledge_normal_y,
        0.0,
        1.0,
    )?;
    nonnegative("physical_hands.climb.fling_carry_s", c.fling_carry_s, 5.0)?;
    positive(
        "physical_hands.climb.tension_full_speed_mps",
        c.tension_full_speed_mps,
        50.0,
    )
}
