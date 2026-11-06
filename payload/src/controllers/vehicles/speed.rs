//! The speed of the vehicle the local character rides, from its world transform across the last
//! world step: what the bindings' parachute bail compares against the animation graph's speed gate
//! ([`PARACHUTE_BAIL_MIN_SPEED_MPS`](crate::controllers::bindings::tables::declare::PARACHUTE_BAIL_MIN_SPEED_MPS)).

use glam::Mat4;
use jc3gi::character::character::Character;

use crate::controllers::physical_hands::physics;

/// The speed of the vehicle the local character is in, metres per second, or `None` outside a
/// vehicle or while it cannot be read.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn seat_vehicle_speed_mps() -> Option<f32> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    let vehicle = unsafe { character.GetVehiclePtr().as_ref() }?;
    let from = Mat4::from(vehicle.m_WorldMatrixT0).w_axis.truncate();
    let to = Mat4::from(vehicle.m_WorldMatrixT1).w_axis.truncate();
    // The two transforms are a world step apart.
    let speed = from.distance(to) / physics::step_seconds();
    speed.is_finite().then_some(speed)
}
