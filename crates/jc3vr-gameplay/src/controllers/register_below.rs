//! Registers the physical hands, the vehicle handling, and the weapon handling with the systems
//! below them: the body, the grip model, the roomscale, the hand bodies' step, the per-shot seam,
//! the weapon take, the weapon pickup hand-off, and the character hooks. Each subsystem's own
//! `register_body` fills its fields of the body's contributions (the vehicle handling's, its
//! take-off and mounted-gun fact, in one), the refusals of the automatic
//! entry into a vehicle's stunt area below the player ride in [`AUTO_ATTACH_BLOCKERS`] in the order
//! they are asked, and the weapon handling's and the physical hands' registrations with each other
//! ride along (`register_siblings`).
//!
//! Each of these does real work every tick. The body reports its own fields still empty
//! ([`jc3vr_body::contributions::register`]), and [`register`] reports the weapon handling's
//! contributions to the install's registration check, which stops the startup naming any still
//! missing.

use jc3vr_body::{contributions::BodyContributions, hooks::character::BlocksAutoAttach};
use jc3vr_core::registration::RegistrationCheck;
use jc3vr_physical::controllers::physical_hands;
use jc3vr_vehicles::controllers::vehicles::rail;
use jc3vr_weapons::{contributions as weapons_contributions, controllers::weapons};

pub(super) fn register(body: &mut BodyContributions, check: &mut RegistrationCheck) {
    physical_hands::register_body::register(body);
    jc3vr_vehicles::register_body(body);
    weapons::register_body::register(body);
    body.auto_attach_blockers = &AUTO_ATTACH_BLOCKERS;
    crate::controllers::register_siblings::register();
    check.require(
        "weapons",
        "the weapon handling's contributions",
        weapons_contributions::registered(),
    );
}

/// The refusals of the automatic entry into a vehicle's stunt area below the player, in the order
/// they are asked: a physical handhold the player climbs by, then the vehicle rail holding him.
static AUTO_ATTACH_BLOCKERS: [BlocksAutoAttach; 2] = [
    physical_hands::register_body::blocks_auto_attach,
    rail::blocks_auto_attach,
];
