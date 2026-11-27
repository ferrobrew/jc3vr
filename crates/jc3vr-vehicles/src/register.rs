//! The vehicles crate's registration: what the vehicle handling contributes to the crates below it,
//! gathered in one [`register`] call, which the payload's composition makes once, before any hook
//! is enabled.
//!
//! - It fills the physical crate's contributions
//!   ([`jc3vr_physical::contributions::PhysicalContributions`]): the vehicle rail's carry in the
//!   hand bodies' pre-simulation step, and reports them to the install's registration check if
//!   they are still missing, which stops the startup naming them.
//! - It fills the vehicle handling's fields of the body's contributions ([`BodyContributions`]):
//!   the wingsuit's take-off from the ground, which the roomscale's frame half ticks, and the
//!   mounted-gun fact ([`jc3vr_body::controllers::facts::mounted`]): the mounted gun the hands aim,
//!   its aim point, and the seated body's aim. The body's registration reports a field left empty.
//! - It returns [`Registered`], the only way to reach the equipment detours, so the composition
//!   cannot enable them without registering the vehicle handling first.
//!
//! The rest of what the vehicle handling contributes rides in the tables of the gameplay crate that
//! declare an order: the vehicle controls' target stage ([`vehicles::apply`], which both the hand
//! composition and the hand bodies' drive run) and the mounted gun's world weapon observer in the
//! hands' tables, and the rail's refusal of the automatic entry into a vehicle's stunt area below
//! the player in the refusals' table. The handles' kick by each shot of the mounted gun the hands
//! aim ([`on_shot`](crate::vehicles::mounted::recoil::on_shot)) is the weapon
//! handling's contribution, which the composition hands over.

use glam::Vec3;
use jc3vr_body::{
    contributions::BodyContributions,
    controllers::{facts::mounted::MountedSource, mode::VehicleSeat},
};
use jc3vr_core::registration::RegistrationCheck;
use jc3vr_physical::contributions::{self as physical, PhysicalContributions};
use re_utilities::hook_library::HookLibrary;

use crate::{
    hooks::equipment,
    vehicles,
    vehicles::{passenger, rail, wingsuit},
};

/// Register the vehicle handling's contributions to the physical crate and fill its fields of
/// `body`, before any hook is enabled, and report the physical crate's to `check` if they are still
/// missing.
pub fn register(body: &mut BodyContributions, check: &mut RegistrationCheck) -> Registered {
    physical::register(PhysicalContributions {
        rail_step: rail::step_physics,
    });
    body.take_off = Some(wingsuit::take_off::tick);
    body.mounted = Some(MountedSource {
        is_mounted_weapon: vehicles::is_mounted_weapon,
        target: vehicles::mounted_target,
        aim_point: vehicles::mounted_aim_point,
        seated_body_aim,
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
        let mut body = BodyContributions::default();
        register(&mut body, &mut check);
        assert_eq!(check.missing(), []);
        assert!(body.take_off.is_some() && body.mounted.is_some());
    }
}
