//! Checks for physical hand handling: the palm shape, pushing, climbing, shoving, picking up
//! objects, and the simulated hand bodies' hits.

use crate::{
    config::validate::{Invalid, nonnegative, positive, pulse, range, vec3},
    controllers::physical_hands::{
        PhysicalHandsConfig,
        climb::ClimbConfig,
        config::{PickupConfig, ShoveConfig},
        physics::{HandBodiesConfig, HandHitsConfig},
    },
};

/// Check the physical hand settings. Paths are relative to `controllers.physical_hands`.
pub(super) fn physical_hands(p: &PhysicalHandsConfig) -> Result<(), Invalid> {
    for (axis, value) in ["x", "y", "z"]
        .into_iter()
        .zip(p.palm_half_extents_m.to_array())
    {
        positive(
            &format!("physical_hands.palm_half_extents_m.{axis}"),
            value,
            0.5,
        )?;
    }
    vec3("physical_hands.palm_offset_m", p.palm_offset_m, 0.5)?;
    nonnegative("physical_hands.push_deadband_m", p.push_deadband_m, 1.0)?;
    nonnegative(
        "physical_hands.push_stiffness_per_s",
        p.push_stiffness_per_s,
        1000.0,
    )?;
    nonnegative(
        "physical_hands.max_push_speed_mps",
        p.max_push_speed_mps,
        20.0,
    )?;
    range(
        "physical_hands.min_push_horizontal",
        p.min_push_horizontal,
        0.0,
        1.0,
    )?;
    positive(
        "physical_hands.full_hit_speed_mps",
        p.full_hit_speed_mps,
        50.0,
    )?;
    nonnegative(
        "physical_hands.min_object_speed_mps",
        p.min_object_speed_mps,
        50.0,
    )?;
    nonnegative("physical_hands.object_impulse", p.object_impulse, 100.0)?;
    pulse("physical_hands.contact_pulse", &p.contact_pulse)?;
    pulse("physical_hands.pressure_pulse", &p.pressure_pulse)?;
    shove(&p.shove)?;
    pickup(&p.pickup)?;
    hand_bodies(&p.bodies)?;
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
    pulse("physical_hands.climb.grab_pulse", &c.grab_pulse)?;
    pulse("physical_hands.climb.release_pulse", &c.release_pulse)?;
    nonnegative("physical_hands.climb.fling_carry_s", c.fling_carry_s, 5.0)?;
    pulse("physical_hands.climb.fling_pulse", &c.fling_pulse)?;
    pulse("physical_hands.climb.tension_pulse", &c.tension_pulse)?;
    positive(
        "physical_hands.climb.tension_full_speed_mps",
        c.tension_full_speed_mps,
        50.0,
    )
}

fn shove(s: &ShoveConfig) -> Result<(), Invalid> {
    nonnegative(
        "physical_hands.shove.stiffness_per_s",
        s.stiffness_per_s,
        1000.0,
    )?;
    nonnegative("physical_hands.shove.deadband_m", s.deadband_m, 1.0)?;
    nonnegative("physical_hands.shove.max_speed_mps", s.max_speed_mps, 50.0)?;
    nonnegative("physical_hands.shove.max_force_n", s.max_force_n, 100_000.0)?;
    nonnegative("physical_hands.shove.friction", s.friction, 10.0)?;
    nonnegative(
        "physical_hands.shove.give_smoothing_s",
        s.give_smoothing_s,
        10.0,
    )
}

fn pickup(p: &PickupConfig) -> Result<(), Invalid> {
    nonnegative("physical_hands.pickup.reach_m", p.reach_m, 1.0)?;
    nonnegative(
        "physical_hands.pickup.max_mass_kg",
        p.max_mass_kg,
        100_000.0,
    )?;
    nonnegative(
        "physical_hands.pickup.hand_force_n",
        p.hand_force_n,
        100_000.0,
    )?;
    nonnegative(
        "physical_hands.pickup.hand_torque_nm",
        p.hand_torque_nm,
        10_000.0,
    )?;
    range(
        "physical_hands.pickup.two_hand_angular_share",
        p.two_hand_angular_share,
        0.0,
        1.0,
    )?;
    nonnegative("physical_hands.pickup.gravity_mps2", p.gravity_mps2, 100.0)?;
    nonnegative(
        "physical_hands.pickup.keep_out_radius_m",
        p.keep_out_radius_m,
        5.0,
    )?;
    nonnegative(
        "physical_hands.pickup.stiffness_per_s",
        p.stiffness_per_s,
        1000.0,
    )?;
    nonnegative(
        "physical_hands.pickup.max_speed_mps",
        p.max_speed_mps,
        200.0,
    )?;
    nonnegative(
        "physical_hands.pickup.angular_stiffness_per_s",
        p.angular_stiffness_per_s,
        1000.0,
    )?;
    nonnegative(
        "physical_hands.pickup.max_angular_speed_rps",
        p.max_angular_speed_rps,
        200.0,
    )?;
    positive(
        "physical_hands.pickup.break_distance_m",
        p.break_distance_m,
        100.0,
    )?;
    nonnegative("physical_hands.pickup.throw_scale", p.throw_scale, 20.0)?;
    nonnegative(
        "physical_hands.pickup.max_throw_speed_mps",
        p.max_throw_speed_mps,
        500.0,
    )?;
    pulse("physical_hands.pickup.grab_pulse", &p.grab_pulse)?;
    pulse("physical_hands.pickup.release_pulse", &p.release_pulse)
}

fn hand_bodies(b: &HandBodiesConfig) -> Result<(), Invalid> {
    range("physical_hands.bodies.strength", b.strength, 0.0, 10.0)?;
    positive("physical_hands.bodies.mass_kg", b.mass_kg, 1000.0)?;
    nonnegative(
        "physical_hands.bodies.max_force_n",
        b.max_force_n,
        1_000_000.0,
    )?;
    nonnegative(
        "physical_hands.bodies.max_torque_nm",
        b.max_torque_nm,
        100_000.0,
    )?;
    range("physical_hands.bodies.linear_gain", b.linear_gain, 0.0, 1.0)?;
    range(
        "physical_hands.bodies.angular_gain",
        b.angular_gain,
        0.0,
        1.0,
    )?;
    range(
        "physical_hands.bodies.contact_gain",
        b.contact_gain,
        0.0,
        1.0,
    )?;
    nonnegative(
        "physical_hands.bodies.push_stiffness_n_per_m",
        b.push_stiffness_n_per_m,
        1_000_000.0,
    )?;
    nonnegative(
        "physical_hands.bodies.max_speed_mps",
        b.max_speed_mps,
        200.0,
    )?;
    nonnegative(
        "physical_hands.bodies.max_angular_speed_rps",
        b.max_angular_speed_rps,
        500.0,
    )?;
    positive("physical_hands.bodies.radius_m", b.radius_m, 0.5)?;
    nonnegative("physical_hands.bodies.half_length_m", b.half_length_m, 0.5)?;
    positive(
        "physical_hands.bodies.break_distance_m",
        b.break_distance_m,
        10.0,
    )?;
    positive(
        "physical_hands.bodies.body_mass_kg",
        b.body_mass_kg,
        10_000.0,
    )?;
    nonnegative(
        "physical_hands.bodies.push_deadband_m",
        b.push_deadband_m,
        1.0,
    )?;
    nonnegative(
        "physical_hands.bodies.max_push_speed_mps",
        b.max_push_speed_mps,
        20.0,
    )?;
    range(
        "physical_hands.bodies.min_push_horizontal",
        b.min_push_horizontal,
        0.0,
        1.0,
    )?;
    nonnegative(
        "physical_hands.bodies.push_smoothing_s",
        b.push_smoothing_s,
        10.0,
    )?;
    positive(
        "physical_hands.bodies.pressure_full_lag_m",
        b.pressure_full_lag_m,
        2.0,
    )?;
    hand_hits(&b.hits)
}

fn hand_hits(h: &HandHitsConfig) -> Result<(), Invalid> {
    nonnegative(
        "physical_hands.bodies.hits.min_impulse_ns",
        h.min_impulse_ns,
        10_000.0,
    )?;
    positive(
        "physical_hands.bodies.hits.knockdown_impulse_ns",
        h.knockdown_impulse_ns,
        10_000.0,
    )?;
    nonnegative(
        "physical_hands.bodies.hits.damage_impulse_ns",
        h.damage_impulse_ns,
        10_000.0,
    )?;
    nonnegative(
        "physical_hands.bodies.hits.damage_scale",
        h.damage_scale,
        100.0,
    )?;
    nonnegative("physical_hands.bodies.hits.cooldown_s", h.cooldown_s, 60.0)?;
    nonnegative(
        "physical_hands.bodies.hits.damage_cooldown_s",
        h.damage_cooldown_s,
        60.0,
    )?;
    nonnegative(
        "physical_hands.bodies.hits.min_approach_speed_mps",
        h.min_approach_speed_mps,
        50.0,
    )?;
    positive(
        "physical_hands.bodies.hits.blow_mass_kg",
        h.blow_mass_kg,
        1000.0,
    )?;
    nonnegative(
        "physical_hands.bodies.hits.follow_through_s",
        h.follow_through_s,
        5.0,
    )?;
    range(
        "physical_hands.bodies.hits.follow_through_gain",
        h.follow_through_gain,
        0.0,
        1.0,
    )?;
    positive(
        "physical_hands.bodies.hits.full_pulse_impulse_ns",
        h.full_pulse_impulse_ns,
        10_000.0,
    )
}
