//! Controller versus gamepad scheme selection from the settings and the published controller
//! frame. The scheme type and its rule live in [`jc3vr_input::scheme`].

use jc3vr_core::config::engine::ConfigSlot;
use jc3vr_input::{
    frame::{self, ControllerFrame, Hand},
    scheme::{Scheme, resolve},
};

/// The controllers' master switch, at `controllers.enabled` in the settings file; the payload
/// binds the slot at startup.
pub static CONTROLLERS_ENABLED: ConfigSlot<bool> = ConfigSlot::new("controllers.enabled");

/// The configured dominant hand, at `controllers.dominant_hand` in the settings file; the payload
/// binds the slot at startup. The hand roles follow [`acting_hand`], which differs from this until
/// a restart after a change.
pub static DOMINANT_HAND: ConfigSlot<Hand> = ConfigSlot::new("controllers.dominant_hand");

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

/// The dominant hand the mod's hand roles follow: the gun hand, the grapple's ray, the readouts on
/// the wrists, the mirrored sticks and levers, and the bindings' reading of the buttons. While
/// controller bindings are attached it is the hand they were suggested for, since OpenXR keeps
/// those until the game restarts and the buttons stay laid out for it; a change of
/// [`DOMINANT_HAND`] takes effect for the roles at the same restart as for the buttons, so the two
/// never disagree.
#[inline]
pub fn acting_hand() -> Hand {
    let dominant = *DOMINANT_HAND.lock();
    acting_hand_for(dominant)
}

/// [`acting_hand`] for a configured `dominant` hand already read from the settings.
#[inline]
pub fn acting_hand_for(dominant: Hand) -> Hand {
    resolve_acting_hand(dominant, jc3vr_render::vr::input::bound_hand())
}

/// The hand the roles follow for a configured `dominant` hand and the hand the attached bindings
/// were suggested for, `bound`, if any.
#[inline]
fn resolve_acting_hand(dominant: Hand, bound: Option<Hand>) -> Hand {
    bound.unwrap_or(dominant)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roles_follow_the_bound_hand_until_a_restart() {
        assert_eq!(resolve_acting_hand(Hand::Left, None), Hand::Left);
        assert_eq!(
            resolve_acting_hand(Hand::Left, Some(Hand::Right)),
            Hand::Right
        );
        assert_eq!(
            resolve_acting_hand(Hand::Right, Some(Hand::Right)),
            Hand::Right
        );
    }
}
