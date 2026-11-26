//! Controller versus gamepad scheme selection from the settings and the published controller
//! frame. The scheme type and its rule live in [`jc3vr_input::scheme`].

use jc3vr_core::config::engine::ConfigSlot;
use jc3vr_input::frame::{self, ControllerFrame};
pub use jc3vr_input::scheme::{Scheme, resolve};

/// The controllers' master switch, at `controllers.enabled` in the settings file; the payload
/// binds the slot at startup.
pub static CONTROLLERS_ENABLED: ConfigSlot<bool> = ConfigSlot::new("controllers.enabled");

/// The scheme in effect now. Cheap: one config read and a peek at the published controller frame.
pub fn current() -> Scheme {
    let enabled = *CONTROLLERS_ENABLED.lock();
    let present = enabled && frame::with_latest(ControllerFrame::any_active).unwrap_or(false);
    resolve(enabled, present)
}

/// Whether the controller scheme is in effect: the gate every controller subsystem checks.
pub fn is_controllers() -> bool {
    current() == Scheme::Controllers
}
