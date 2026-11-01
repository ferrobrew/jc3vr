//! Checks for vehicle settings: the wingsuit gestures, the physical wheel, the arm steering, and
//! the parachute handles.

use crate::{
    config::validate::{Invalid, nonnegative, positive, range},
    controllers::vehicles::config::VehiclesConfig,
};

/// Check the vehicle settings. Paths are relative to `controllers.vehicles`.
pub(super) fn vehicles(v: &VehiclesConfig) -> Result<(), Invalid> {
    if !v.wingsuit_gestures.valid() {
        return Err(Invalid(
            "vehicles.wingsuit_gestures must have finite, bounded, consistent thresholds".into(),
        ));
    }
    range(
        "vehicles.physical.wheel.lock_deg",
        v.physical.wheel.lock_deg,
        1.0,
        1080.0,
    )?;
    range(
        "vehicles.physical.wheel.response",
        v.physical.wheel.response,
        0.2,
        5.0,
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
    range(
        "vehicles.arm_steering.tilt_neutral_deg",
        v.arm_steering.tilt_neutral_deg,
        -90.0,
        90.0,
    )?;
    positive(
        "vehicles.parachute_handles.full_pull_m",
        v.parachute_handles.full_pull_m,
        10.0,
    )?;
    range(
        "vehicles.parachute_handles.brake_threshold",
        v.parachute_handles.brake_threshold,
        0.0,
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use crate::{config::validate::controllers, controllers::config::ControllersConfig};

    #[test]
    fn invalid_wingsuit_thresholds_reach_the_controller_validator() {
        let mut config = ControllersConfig::default();
        assert!(controllers(&config).is_ok());
        config.vehicles.wingsuit_gestures.enabled = false;
        config.vehicles.wingsuit_gestures.brake_roll_deg = f32::NAN;
        let error = controllers(&config).unwrap_err();
        assert!(error.0.starts_with("vehicles.wingsuit_gestures "));
    }
}
