//! Registers the weapon handling and the physical hands with each other, which neither names: the
//! grip view's flash for a refused take of a weapon, and the props the hands carry, which the
//! loadout table records.

use jc3vr_input::frame::Hand;
use jc3vr_physical::controllers::physical_hands::{grab_view, held_props};

use crate::controllers::weapons::{
    hold::take::register_refusal_sink, inventory::register_held_props,
};

pub(super) fn register() {
    register_refusal_sink(take_refused);
    register_held_props(held_props);
}

/// A refused take of a weapon, flashed in the grip view at the target the hand's grip chose.
fn take_refused(hand: Hand, why: &str) {
    grab_view::refused(hand, None, why);
}
