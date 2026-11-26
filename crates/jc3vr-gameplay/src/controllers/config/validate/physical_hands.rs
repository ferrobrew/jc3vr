//! Checks for physical hand handling: the hands' strength.

use jc3vr_core::config::engine::{Invalid, range};
use jc3vr_physical::controllers::physical_hands::PhysicalHandsConfig;

/// Check the physical hand settings. Paths are relative to `controllers.physical_hands`.
pub(super) fn physical_hands(p: &PhysicalHandsConfig) -> Result<(), Invalid> {
    range(
        "physical_hands.bodies.strength",
        p.bodies.strength,
        0.0,
        10.0,
    )
}
