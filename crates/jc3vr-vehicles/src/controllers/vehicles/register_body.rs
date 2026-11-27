//! Registers the vehicle handling with the systems below it: the wingsuit's take-off from the
//! ground, ticked by the roomscale's frame half; the vehicle rail's refusal of the automatic entry
//! into a vehicle's stunt area below the player; the rail's carry in the hand bodies'
//! pre-simulation step; and the vehicle controls' targets in the hand bodies' drive. The handles'
//! kick by each shot of the mounted gun the hands aim ([`mounted_recoil::on_shot`]) is the weapon
//! handling's contribution, which the gameplay crate's install registers.

use jc3vr_body::{
    controllers::roomscale::register_take_off,
    hooks::character::{AutoAttachBlocker, register_auto_attach_blocker},
};
use jc3vr_physical::controllers::physical_hands::physics::{
    guns::targets::register_vehicle_targets, register_rail_step,
};

use crate::controllers::{
    vehicles,
    vehicles::{rail, take_off},
};

pub fn register() {
    register_take_off(take_off::tick);
    register_auto_attach_blocker(AutoAttachBlocker::VehicleRail, rail::blocks_auto_attach);
    register_rail_step(rail::step_physics);
    register_vehicle_targets(vehicles::apply);
}
