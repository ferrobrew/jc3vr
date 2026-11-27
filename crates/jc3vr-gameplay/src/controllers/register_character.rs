//! Registers the refusals of the automatic entry into a vehicle's stunt area below the player with
//! the character hooks, in the order they are asked ([`AUTO_ATTACH_BLOCKERS`]). The field is one of
//! the body's contributions, which reports it still empty to the install's registration check
//! ([`jc3vr_body::contributions::register`]).

use jc3vr_body::{contributions::BodyContributions, hooks::character::BlocksAutoAttach};
use jc3vr_physical::controllers::physical_hands;
use jc3vr_vehicles::vehicles::rail;

pub(super) fn register(body: &mut BodyContributions) {
    body.auto_attach_blockers = &AUTO_ATTACH_BLOCKERS;
}

/// The refusals of the automatic entry into a vehicle's stunt area below the player, in the order
/// they are asked: a physical handhold the player climbs by, then the vehicle rail holding him.
static AUTO_ATTACH_BLOCKERS: [BlocksAutoAttach; 2] = [
    physical_hands::register_body::blocks_auto_attach,
    rail::blocks_auto_attach,
];
