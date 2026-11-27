//! Registers the vehicle handling with the body: the wingsuit's take-off from the ground, ticked by
//! the roomscale's frame half; and the vehicle rail's refusal of the automatic entry into a
//! vehicle's stunt area below the player. The rail's carry and the vehicle controls' targets are
//! the physical crate's contributions, which the crate's own registration fills
//! ([`crate::register()`]). The handles' kick by each shot of the mounted gun the hands aim
//! ([`on_shot`](crate::controllers::vehicles::mounted_recoil::on_shot)) is the weapon handling's
//! contribution, which the gameplay crate's install registers.

use jc3vr_body::{
    controllers::roomscale::register_take_off,
    hooks::character::{AutoAttachBlocker, register_auto_attach_blocker},
};

use crate::controllers::vehicles::{rail, take_off};

pub fn register() {
    register_take_off(take_off::tick);
    register_auto_attach_blocker(AutoAttachBlocker::VehicleRail, rail::blocks_auto_attach);
}
