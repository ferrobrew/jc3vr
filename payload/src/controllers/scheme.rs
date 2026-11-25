//! Controller versus gamepad scheme selection.
//!
//! The controller scheme (hand aiming, the controller bindings, controller-owned hands) is in
//! effect while the controllers are enabled and a controller is active in the published controller
//! frame. Otherwise (no session, controllers disabled, or no controller tracked) the gamepad scheme
//! (gaze aiming, the game's own gamepad and keyboard input, animated hands) is in effect, so the
//! mod's behaviour without controllers is unchanged.

use crate::{
    config::Config,
    controllers::frame::{self, ControllerFrame},
};

/// The control scheme in effect.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Scheme {
    /// Hand aiming, the controller bindings, and controller-owned hands.
    Controllers,
    /// Gaze aiming, the game's own gamepad and keyboard input, and animated hands.
    Gamepad,
}

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

/// The scheme in effect for the controllers' master switch and whether any controller is present.
pub fn resolve(enabled: bool, controllers_present: bool) -> Scheme {
    if enabled && controllers_present {
        Scheme::Controllers
    } else {
        Scheme::Gamepad
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_controller_scheme_needs_the_switch_and_a_controller() {
        assert_eq!(resolve(true, true), Scheme::Controllers);
        assert_eq!(resolve(true, false), Scheme::Gamepad);
        assert_eq!(resolve(false, true), Scheme::Gamepad);
        assert_eq!(resolve(false, false), Scheme::Gamepad);
    }
}
