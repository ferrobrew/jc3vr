//! The vehicles crate's registration: what the vehicle handling contributes to the crates below it,
//! gathered in one place.
//!
//! [`register`] fills the physical crate's contributions
//! ([`jc3vr_physical::contributions::PhysicalContributions`]): the vehicle rail's carry in the hand
//! bodies' pre-simulation step. It reports them to the install's registration check if they are
//! still missing, which stops the startup naming them. The body's registrations are still in
//! [`register_body`](crate::controllers::vehicles::register_body) and
//! [`register_mounted`](crate::controllers::vehicles::register_mounted), which the gameplay crate's
//! install calls.
//!
//! It returns [`Registered`], the only way to reach the equipment detours, so the payload's
//! composition cannot enable them without registering the vehicle handling first.

use jc3vr_core::registration::RegistrationCheck;
use jc3vr_physical::contributions::{self as physical, PhysicalContributions};
use re_utilities::hook_library::HookLibrary;

use crate::{controllers::vehicles::rail, hooks::equipment};

/// Register the vehicle handling's contributions to the crates below it, before any hook is
/// enabled, and report each one still missing to `check`.
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
