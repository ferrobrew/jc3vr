//! Y under a helicopter: the way up onto its skid from the line's hang below the airframe.
//!
//! On foot, Y asks for the use, the vehicle entry, and the wingsuit at once. Hanging on the line
//! below a helicopter, the entry has no state to take it, and the wingsuit opened and ended the
//! reel instead (1800 s on the 14 November take). So while the lock hangs Rico below a helicopter,
//! Y's wingsuit is dropped and its press asks the lock to point the reel at the door side and let
//! it arrive ([`lock::entering_helicopter`]), which pulls Rico up onto the skid. The press that
//! started the pull is kept from the game until it is released, so the door-side arrival leaves
//! him standing on the skid; a fresh press enters from there.

use std::sync::atomic::{AtomicBool, Ordering};

use jc3gi::input::input_action_map::Action;

use crate::{
    controllers::bindings::hold::{Desired, MapKind, Want},
    grapple::physical::lock,
};

/// Rework `desired` for a hang below a helicopter; returns whether Y was pressed in the hang.
pub(super) fn step(desired: &mut Vec<Desired>) -> bool {
    let hanging = lock::hangs_under_helicopter();
    let entering = lock::entering_helicopter();
    let entry_held = desired
        .iter()
        .any(|d| is_player(d, Action::ENTER_VEHICLE) && presses(d.want));
    if !entry_held {
        SWALLOW.store(false, Ordering::Relaxed);
    }
    if !hanging && !entering && !SWALLOW.load(Ordering::Relaxed) {
        return false;
    }
    if hanging || entering {
        desired.retain(|d| !is_player(d, Action::OPEN_WINGSUIT));
    }
    let pressed = hanging && entry_held;
    if pressed {
        SWALLOW.store(true, Ordering::Relaxed);
    }
    if SWALLOW.load(Ordering::Relaxed) {
        desired.retain(|d| !is_player(d, Action::ENTER_VEHICLE));
    }
    pressed
}

fn is_player(desired: &Desired, action: Action) -> bool {
    desired.target.map == MapKind::Player && desired.target.action == action
}

fn presses(want: Want) -> bool {
    match want {
        Want::Click => true,
        Want::Hold(value) | Want::Analog(value) => value.is_finite() && value >= 0.5,
    }
}

/// Set from the press that starts the pull until its release: the entry is kept from the game.
static SWALLOW: AtomicBool = AtomicBool::new(false);
