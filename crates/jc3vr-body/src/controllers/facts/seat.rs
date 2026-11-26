//! Whether the player rides in or stands in a seat the grapple fires from: the game's own tests,
//! which the seat grapple (`grapple::seat`) and the systems below it (the aim, the hand
//! bodies) read alike.
//!
//! Both tests read only the game's state, so they live here rather than behind a published copy:
//! every reader asks the game at the moment of the read, as the seat grapple does.

use jc3gi::{character::character::Character, equipment::grappling_hook::GrapplingHook};

/// Whether `character` rides in a vehicle seat, as the game's own vehicle grapple tests it: it has
/// a vehicle, and is in one of the riding states ([`Character::IsInRidingInVehicleState`]). The one
/// test of being in a seat for the seat grapple's button, aim, and hook, and for the passenger's
/// body aim.
///
/// # Safety
///
/// `character` must be live, on the game thread.
///
/// [`Character::IsInRidingInVehicleState`]: jc3gi::character::character::Character::IsInRidingInVehicleState
pub unsafe fn rides_in_seat(character: &Character) -> bool {
    !unsafe { character.GetVehiclePtr() }.is_null()
        && unsafe { character.IsInRidingInVehicleState() }
}

/// Whether the seat grapple stands in for the game's grapple for `character` with its hook `hook`:
/// it rides in a seat ([`rides_in_seat`]) and the game refuses the hook's fire there.
///
/// # Safety
///
/// `character` and `hook` must be live, on the game thread.
pub unsafe fn stands_in(character: &Character, hook: &GrapplingHook) -> bool {
    unsafe { !hook.GetFireInputEnabled() && rides_in_seat(character) }
}
