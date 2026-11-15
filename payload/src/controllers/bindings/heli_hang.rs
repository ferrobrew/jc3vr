//! Y under a helicopter: the way into it from the line's hang below the airframe.
//!
//! On foot, Y asks for the use, the vehicle entry, and the wingsuit at once. Hanging on the line
//! below a helicopter, the entry has no state to take it (the line's hang stands in for the game's
//! hang below the airframe, the one state that accepts it), and the wingsuit opened and ended the
//! reel instead (1800 s on the 14 November take). So while the lock hangs Rico below a helicopter,
//! Y's wingsuit is dropped and its press asks the lock to let the reel pick its entry afresh and
//! arrive, which pulls Rico up to the door side ([`lock::entering_helicopter`]), and for that
//! window the entry is held, so the arrival's state takes it and enters.

use jc3gi::input::input_action_map::Action;

use crate::{
    controllers::bindings::hold::{Desired, MapKind, Target, Want},
    grapple::physical::lock,
};

/// Rework `desired` for a hang below a helicopter; returns whether Y was pressed in the hang.
pub(super) fn step(desired: &mut Vec<Desired>) -> bool {
    let hanging = lock::hangs_under_helicopter();
    let entering = lock::entering_helicopter();
    if !hanging && !entering {
        return false;
    }
    let pressed = desired
        .iter()
        .any(|d| is_player(d, Action::ENTER_VEHICLE) && presses(d.want));
    desired.retain(|d| !is_player(d, Action::OPEN_WINGSUIT));
    if entering && !desired.iter().any(|d| is_player(d, Action::ENTER_VEHICLE)) {
        desired.push(Desired {
            target: Target {
                map: MapKind::Player,
                action: Action::ENTER_VEHICLE,
            },
            want: Want::Hold(1.0),
        });
    }
    hanging && pressed
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
