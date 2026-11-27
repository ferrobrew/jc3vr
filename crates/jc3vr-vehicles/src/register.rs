//! The vehicles crate's registration: what the vehicle handling contributes to the crates below it,
//! gathered in one place.
//!
//! - [`register`] fills the physical crate's contributions
//!   ([`jc3vr_physical::contributions::PhysicalContributions`]): the vehicle rail's carry in the
//!   hand bodies' pre-simulation step. It reports them to the install's registration check if they
//!   are still missing, which stops the startup naming them, and returns [`Registered`], the only way to reach the equipment
//!   detours, so the payload's composition cannot enable them without registering the vehicle
//!   handling first.
//! - [`register_body`] fills the vehicle handling's fields of the body's contributions
//!   ([`BodyContributions`]): the wingsuit's take-off from the ground, which the roomscale's frame
//!   half ticks, and the mounted-gun fact ([`jc3vr_body::controllers::facts::mounted`]): the
//!   mounted gun the hands aim, its aim point, and the seated body's aim. The body's registration
//!   reports a field left empty.
//!
//! The rest of what the vehicle handling contributes rides in the tables of the crate above that
//! declare an order: the vehicle controls' target stage ([`vehicles::apply`], which both the hand
//! composition and the hand bodies' drive run) and the mounted gun's world weapon observer in the
//! hands' tables, and the rail's refusal of the automatic entry into a vehicle's stunt area
//! below the player in the refusals' table. The handles' kick by each shot of the mounted gun the
//! hands aim ([`on_shot`](crate::controllers::vehicles::mounted_recoil::on_shot)) is the weapon
//! handling's contribution, which the crate above hands over.

use glam::Vec3;
use jc3vr_body::{
    contributions::BodyContributions,
    controllers::{facts::mounted::MountedSource, mode::VehicleSeat},
};
use jc3vr_core::registration::RegistrationCheck;
use jc3vr_physical::contributions::{self as physical, PhysicalContributions};
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{
        vehicles,
        vehicles::{passenger, rail, take_off},
    },
    hooks::equipment,
};

/// Register the vehicle handling's contributions to the physical crate, before any hook is
/// enabled, and report them to `check` if they are still missing.
pub fn register(check: &mut RegistrationCheck) -> Registered {
    physical::register(PhysicalContributions {
        rail_step: rail::step_physics,
    });
    check.require(
        "hand bodies",
        "the physical crate's contributions",
        physical::registered(),
    );
    Registered {
        equipment: equipment::hook_library,
    }
}

/// What the vehicles crate hands the payload's composition once its contributions are registered.
pub struct Registered {
    equipment: fn() -> HookLibrary,
}

impl Registered {
    /// The equipment's weapon update detours and native start-using guard
    /// ([`crate::hooks::equipment`]), which the composition enables in its own place in the order.
    pub fn equipment_hooks(&self) -> fn() -> HookLibrary {
        self.equipment
    }
}

/// Fill the vehicle handling's fields of `body`: the wingsuit's take-off and the mounted-gun
/// source.
pub fn register_body(body: &mut BodyContributions) {
    body.take_off = Some(take_off::tick);
    body.mounted = Some(MountedSource {
        is_mounted_weapon: vehicles::is_mounted_weapon,
        target: vehicles::mounted_target,
        aim_point: vehicles::mounted_aim_point,
        seated_body_aim,
    });
}

/// The seated body's aim point, and the record of whether this update set one.
fn seated_body_aim(
    seat: Option<VehicleSeat>,
    mounted: bool,
    gun_targets: [Option<Vec3>; 2],
) -> Option<Vec3> {
    let point = passenger::body_aim_point(seat, mounted, gun_targets);
    passenger::note(point.is_some());
    point
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registration_leaves_nothing_missing() {
        let mut check = RegistrationCheck::new();
        register(&mut check);
        assert_eq!(check.missing(), []);
    }
}
