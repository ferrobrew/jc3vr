//! The speed of the vehicle the local character rides, as the animation graph's parachute bail
//! condition measures it: what the bindings' bail compares against the graph's speed gate
//! ([`PARACHUTE_BAIL_MIN_SPEED_MPS`](crate::controllers::bindings::bail::PARACHUTE_BAIL_MIN_SPEED_MPS)).

use glam::{Mat4, Vec3};
use jc3gi::{character::character::Character, types::math::Vector3};

/// The speed of the vehicle the local character is in, metres per second, or `None` outside a
/// vehicle or while it cannot be read: the length of the vehicle's ground-relative velocity at the
/// character's position ([`Vehicle::GetGroundRelativePointVelocity`]), which is what the graph's
/// parent-vehicle speed condition compares against its bounds.
///
/// [`Vehicle::GetGroundRelativePointVelocity`]: jc3gi::vehicle::vehicle::Vehicle::GetGroundRelativePointVelocity
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn seat_vehicle_speed_mps() -> Option<f32> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    let vehicle = unsafe { character.GetVehiclePtr().as_ref() }?;
    // The game reads the velocity through the physics body without checking it.
    if vehicle.m_PfxVehicle.is_null() {
        return None;
    }
    let point = Vector3 {
        data: Mat4::from(character.m_WorldMatrixT1)
            .w_axis
            .truncate()
            .to_array(),
    };
    let mut velocity = Vector3 { data: [0.0; 3] };
    unsafe { vehicle.GetGroundRelativePointVelocity(&point, &mut velocity) };
    let speed = Vec3::from_array(velocity.data).length();
    speed.is_finite().then_some(speed)
}
