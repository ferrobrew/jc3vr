//! Checks for vehicle settings: the mounted grab, the physical wheel, the handlebars and
//! aircraft controls, the arm steering, the driving view, and the parachute handles.

use glam::Vec3;

use crate::{
    config::validate::{Invalid, nonnegative, positive, range, vec3},
    controllers::vehicles::config::{VehiclesConfig, WheelSpec},
};

/// Check the vehicle settings. Paths are relative to `controllers.vehicles`.
pub(super) fn vehicles(v: &VehiclesConfig) -> Result<(), Invalid> {
    positive("vehicles.mounted.min_lever_m", v.mounted.min_lever_m, 10.0)?;
    physical_wheel(&v.physical.wheel)?;
    vec3(
        "vehicles.physical.handlebars.centre_m",
        v.physical.handlebars.centre_m,
        10.0,
    )?;
    positive(
        "vehicles.physical.handlebars.half_width_m",
        v.physical.handlebars.half_width_m,
        10.0,
    )?;
    range(
        "vehicles.physical.handlebars.max_steer_deg",
        v.physical.handlebars.max_steer_deg,
        0.0,
        360.0,
    )?;
    range(
        "vehicles.physical.handlebars.max_lean_deg",
        v.physical.handlebars.max_lean_deg,
        0.0,
        360.0,
    )?;
    stick(
        "vehicles.physical.cyclic.top_m",
        v.physical.cyclic.top_m,
        v.physical.cyclic.max_deflection_m,
    )?;
    lever(
        "vehicles.physical.collective.rest_m",
        v.physical.collective.rest_m,
        v.physical.collective.travel_m,
        v.physical.collective.detent,
    )?;
    stick(
        "vehicles.physical.flight_stick.top_m",
        v.physical.flight_stick.top_m,
        v.physical.flight_stick.max_deflection_m,
    )?;
    lever(
        "vehicles.physical.throttle.rest_m",
        v.physical.throttle.rest_m,
        v.physical.throttle.travel_m,
        v.physical.throttle.detent,
    )?;
    positive(
        "vehicles.physical.grab_radius_m",
        v.physical.grab_radius_m,
        10.0,
    )?;
    positive(
        "vehicles.physical.release_distance_m",
        v.physical.release_distance_m,
        10.0,
    )?;
    range(
        "vehicles.arm_steering.min_spread",
        v.arm_steering.min_spread,
        0.0,
        2.0,
    )?;
    nonnegative(
        "vehicles.arm_steering.deadzone_m",
        v.arm_steering.deadzone_m,
        10.0,
    )?;
    positive(
        "vehicles.arm_steering.full_bank_m",
        v.arm_steering.full_bank_m,
        10.0,
    )?;
    positive(
        "vehicles.arm_steering.full_pitch_m",
        v.arm_steering.full_pitch_m,
        10.0,
    )?;
    range(
        "vehicles.arm_steering.full_tilt_deg",
        v.arm_steering.full_tilt_deg,
        1.0,
        180.0,
    )?;
    range(
        "vehicles.arm_steering.tilt_deadzone_deg",
        v.arm_steering.tilt_deadzone_deg,
        0.0,
        90.0,
    )?;
    range(
        "vehicles.arm_steering.tilt_neutral_deg",
        v.arm_steering.tilt_neutral_deg,
        -90.0,
        90.0,
    )?;
    positive(
        "vehicles.arm_steering.full_sweep_m",
        v.arm_steering.full_sweep_m,
        10.0,
    )?;
    vec3(
        "vehicles.driving_view.hub_from_eyes_m",
        v.driving_view.hub_from_eyes_m,
        2.0,
    )?;
    nonnegative("vehicles.driving_view.max_m", v.driving_view.max_m, 1.0)?;
    nonnegative(
        "vehicles.driving_view.blend_s",
        v.driving_view.blend_s,
        10.0,
    )?;
    vec3(
        "vehicles.parachute_handles.from_shoulder_m",
        v.parachute_handles.from_shoulder_m,
        10.0,
    )?;
    positive(
        "vehicles.parachute_handles.radius_m",
        v.parachute_handles.radius_m,
        10.0,
    )?;
    positive(
        "vehicles.parachute_handles.reach_m",
        v.parachute_handles.reach_m,
        10.0,
    )?;
    positive(
        "vehicles.parachute_handles.full_pull_m",
        v.parachute_handles.full_pull_m,
        10.0,
    )?;
    nonnegative(
        "vehicles.parachute_handles.deadzone_m",
        v.parachute_handles.deadzone_m,
        10.0,
    )?;
    range(
        "vehicles.parachute_handles.brake_threshold",
        v.parachute_handles.brake_threshold,
        0.0,
        1.0,
    )?;
    positive(
        "vehicles.parachute_handles.release_distance_m",
        v.parachute_handles.release_distance_m,
        10.0,
    )
}

fn physical_wheel(value: &WheelSpec) -> Result<(), Invalid> {
    vec3("vehicles.physical.wheel.centre_m", value.centre_m, 10.0)?;
    positive("vehicles.physical.wheel.radius_m", value.radius_m, 10.0)?;
    range(
        "vehicles.physical.wheel.tilt_deg",
        value.tilt_deg,
        -180.0,
        180.0,
    )?;
    range(
        "vehicles.physical.wheel.lock_deg",
        value.lock_deg,
        1.0,
        1080.0,
    )?;
    range("vehicles.physical.wheel.response", value.response, 0.2, 5.0)?;
    range(
        "vehicles.physical.wheel.deadzone_deg",
        value.deadzone_deg,
        0.0,
        90.0,
    )?;
    nonnegative(
        "vehicles.physical.wheel.return_deg_per_s",
        value.return_deg_per_s,
        10_000.0,
    )
}

fn stick(name: &str, position: Vec3, deflection: f32) -> Result<(), Invalid> {
    vec3(name, position, 10.0)?;
    positive(&format!("{name}.max_deflection_m"), deflection, 10.0)
}

fn lever(name: &str, position: Vec3, travel: f32, detent: f32) -> Result<(), Invalid> {
    vec3(name, position, 10.0)?;
    positive(&format!("{name}.travel_m"), travel, 10.0)?;
    range(&format!("{name}.detent"), detent, 0.0, 1.0)
}
