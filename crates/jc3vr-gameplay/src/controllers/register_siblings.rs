//! Registers what the weapon handling reads from the crates beside and above it, which it names
//! none of ([`jc3vr_weapons::contributions`]): the handles' kick by each shot of the mounted gun
//! the hands aim, the grip view's flash for a refused take of a weapon, and the props the hands
//! carry, which the loadout table records.

use jc3vr_input::frame::Hand;
use jc3vr_physical::controllers::physical_hands::{grab_view, held_props};
use jc3vr_vehicles::controllers::vehicles::mounted_recoil;
use jc3vr_weapons::contributions::{self, WeaponsContributions};

pub(super) fn register() {
    contributions::register(WeaponsContributions {
        mounted_shot: mounted_recoil::on_shot,
        take_refused,
        held_props,
    });
}

/// A refused take of a weapon, flashed in the grip view at the target the hand's grip chose.
fn take_refused(hand: Hand, why: &str) {
    grab_view::refused(hand, None, why);
}
