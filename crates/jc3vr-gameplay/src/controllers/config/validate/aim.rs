//! Checks for the controller-owned hands and per-hand aiming.

use jc3vr_body::controllers::{aim::config::AimConfig, hands::config::HandsConfig};
use jc3vr_core::config::engine::{Invalid, range, vec3};

/// Check the controller-owned hands' settings. Paths are relative to `controllers.hands`.
pub(super) fn hands(c: &HandsConfig) -> Result<(), Invalid> {
    vec3("hands.grip.position_m", c.grip.position_m, 5.0)?;
    vec3("hands.grip.rotation_deg", c.grip.rotation_deg, 360.0)
}

/// Check the per-hand aiming settings. Paths are relative to `controllers.aim`.
pub(super) fn aim(c: &AimConfig) -> Result<(), Invalid> {
    range("aim.assist_strength", c.assist_strength, 0.0, 1.0)
}
