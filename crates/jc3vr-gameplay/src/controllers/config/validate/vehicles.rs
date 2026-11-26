//! Checks for the vehicle settings: the wheel's lock and the wingsuit's neutral wrist tilt.

use jc3vr_core::config::engine::{Invalid, range};
use jc3vr_vehicles::controllers::vehicles::config::VehiclesConfig;

/// Check the vehicle settings. Paths are relative to `controllers.vehicles`.
pub(super) fn vehicles(v: &VehiclesConfig) -> Result<(), Invalid> {
    range(
        "vehicles.physical.wheel.lock_deg",
        v.physical.wheel.lock_deg,
        1.0,
        1080.0,
    )?;
    range(
        "vehicles.arm_steering.tilt_neutral_deg",
        v.arm_steering.tilt_neutral_deg,
        -90.0,
        90.0,
    )
}
