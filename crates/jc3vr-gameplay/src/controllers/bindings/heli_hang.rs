//! Y under a helicopter: the way up onto its skid from the line's hang below the airframe.
//!
//! On foot, Y asks for the use, the vehicle entry, and the wingsuit at once. Hanging on the line
//! below a helicopter, the entry has no state to take it, and the wingsuit opened and ended the
//! reel instead. So while the lock hangs Rico below a helicopter,
//! Y's wingsuit is dropped and its press asks the lock to point the reel at the door side and let
//! it arrive ([`lock::entering_helicopter`]), which pulls Rico up onto the skid. The press that
//! started the pull is kept from the game until it is released, so the door-side arrival leaves
//! him standing on the skid; a fresh press enters from there.

use jc3gi::input::input_action_map::Action;
use jc3vr_body::controllers::intent::hold::{Desired, MapKind, Want};
use jc3vr_grapple::grapple::physical::lock;

use crate::controllers::tuning::GAMEPLAY;

/// The hang's hold on the entry press.
pub(super) struct HeliHang {
    /// Set from the press that starts the pull until its release: the entry is kept from the game.
    swallow: bool,
}

impl HeliHang {
    pub(super) const fn new() -> Self {
        Self { swallow: false }
    }

    /// Rework `desired` for a hang below a helicopter; returns whether Y was pressed in the hang.
    pub(super) fn step(&mut self, desired: &mut Vec<Desired>) -> bool {
        let hanging = lock::hangs_under_helicopter();
        let entering = lock::entering_helicopter();
        let entry_held = desired
            .iter()
            .any(|d| is_player(d, Action::ENTER_VEHICLE) && presses(d.want));
        if !entry_held {
            self.swallow = false;
        }
        if !hanging && !entering && !self.swallow {
            return false;
        }
        if hanging || entering {
            desired.retain(|d| !is_player(d, Action::OPEN_WINGSUIT));
        }
        let pressed = hanging && entry_held;
        if pressed {
            self.swallow = true;
        }
        if self.swallow {
            desired.retain(|d| !is_player(d, Action::ENTER_VEHICLE));
        }
        pressed
    }
}

fn is_player(desired: &Desired, action: Action) -> bool {
    desired.target.map == MapKind::Player && desired.target.action == action
}

fn presses(want: Want) -> bool {
    match want {
        Want::Click => true,
        Want::Hold(value) | Want::Analog(value) => {
            value.is_finite() && value >= GAMEPLAY.get().bindings.heli_hang.press
        }
    }
}
