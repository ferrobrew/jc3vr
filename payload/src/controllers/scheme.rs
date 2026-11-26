//! Controller versus gamepad scheme selection from the settings and the published controller
//! frame. The scheme type and its rule live in [`crate::input_core::scheme`].

pub(crate) use crate::input_core::scheme::{Scheme, resolve};
use crate::{
    config::Config,
    controllers::frame::{self, ControllerFrame},
};

/// The scheme in effect now. Cheap: one config read and a peek at the published controller frame.
pub fn current() -> Scheme {
    let enabled = Config::lock_query(|c| c.controllers.enabled);
    let present = enabled && frame::with_latest(ControllerFrame::any_active).unwrap_or(false);
    resolve(enabled, present)
}

/// Whether the controller scheme is in effect: the gate every controller subsystem checks.
pub fn is_controllers() -> bool {
    current() == Scheme::Controllers
}
