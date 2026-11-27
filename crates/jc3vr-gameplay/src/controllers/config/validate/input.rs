//! Checks for the controller-to-game bindings.

use jc3vr_body::controllers::intent::config::BindingsConfig;
use jc3vr_core::config::{Invalid, range};

/// Check the controller-to-game bindings. Paths are relative to `controllers.bindings`.
pub(super) fn bindings(c: &BindingsConfig) -> Result<(), Invalid> {
    range("bindings.move_deadzone", c.move_deadzone, 0.0, 1.0)
}
