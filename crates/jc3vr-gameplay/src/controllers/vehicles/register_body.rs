//! Registers the vehicle handling with the systems below it: the wingsuit's take-off from the
//! ground, ticked by the roomscale's frame half; the vehicle rail's refusal of the automatic entry
//! into a vehicle's stunt area below the player; the rail's carry in the hand bodies'
//! pre-simulation step; and the handles' kick by each shot of the mounted gun the hands aim.

use crate::{
    controllers::{
        physical_hands::physics::register_rail_step,
        roomscale::register_take_off,
        vehicles::{mounted_recoil, rail, take_off},
        weapons::fire::register_mounted_shot_observer,
    },
    hooks::character::{AutoAttachBlocker, register_auto_attach_blocker},
};

pub(crate) fn register() {
    register_take_off(take_off::tick);
    register_auto_attach_blocker(AutoAttachBlocker::VehicleRail, rail::blocks_auto_attach);
    register_rail_step(rail::step_physics);
    register_mounted_shot_observer(mounted_recoil::on_shot);
}
