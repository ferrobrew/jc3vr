//! Registers the vehicle handling with the roomscale and the character hooks below it: the
//! wingsuit's take-off from the ground, ticked by the roomscale's frame half, and the vehicle
//! rail's refusal of the automatic entry into a vehicle's stunt area below the player.

use crate::{
    controllers::{
        roomscale::register_take_off,
        vehicles::{rail, take_off},
    },
    hooks::character::{AutoAttachBlocker, register_auto_attach_blocker},
};

pub(crate) fn register() {
    register_take_off(take_off::tick);
    register_auto_attach_blocker(AutoAttachBlocker::VehicleRail, rail::blocks_auto_attach);
}
