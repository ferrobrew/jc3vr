//! The vehicle behind a physics game object or a body: the vehicle a ray hit on a vehicle or one of
//! its loosened parts belongs to, the vehicle that owns a body, the vehicle's native rigid body, and
//! its world transform.
//!
//! Every function here runs on the game thread outside the physics step.

use std::num::NonZeroUsize;

use glam::Mat4;
use jc3gi::{
    physics::PfxRigidBody,
    vehicle::{parts::VehiclePartGameObject, vehicle::Vehicle},
};

use crate::controllers::physics::bodies::{is_type, owner_of};

/// The vehicle a ray hit on `object` belongs to: the vehicle itself, or the vehicle a loosened
/// part such as an open door came from.
///
/// # Safety
///
/// `object` must be a live physics game object, from a cast this tick.
#[inline]
pub unsafe fn vehicle_of(object: NonZeroUsize) -> Option<NonZeroUsize> {
    if unsafe { is_type(object, Vehicle::TYPE_ID()) } {
        return Some(object);
    }
    if !unsafe { is_type(object, VehiclePartGameObject::TYPE_ID()) } {
        return None;
    }
    let part = object.get() as *mut VehiclePartGameObject;
    NonZeroUsize::new(unsafe { (*part).GetVehicle() } as usize)
}

/// Whether the physics game object at `object` is a loosened vehicle part.
///
/// # Safety
///
/// `object` must be a live physics game object.
#[inline]
pub unsafe fn is_vehicle_part(object: NonZeroUsize) -> bool {
    unsafe { is_type(object, VehiclePartGameObject::TYPE_ID()) }
}

/// The vehicle that owns the body `body`, if a vehicle does.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub unsafe fn vehicle_owning(body: u32) -> Option<NonZeroUsize> {
    let owner = unsafe { owner_of(body) }?;
    unsafe { is_type(owner, Vehicle::TYPE_ID()) }.then_some(owner)
}

/// The vehicle's world transform at the end of the current simulation step.
///
/// # Safety
///
/// `vehicle` must be a live vehicle.
#[inline]
pub unsafe fn vehicle_frame(vehicle: NonZeroUsize) -> Mat4 {
    let vehicle = unsafe { &*(vehicle.get() as *const Vehicle) };
    Mat4::from(vehicle.m_WorldMatrixT1)
}

/// Whether `vehicle` still owns its body `vehicle_body`, which proves it is alive.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub unsafe fn owns_body(vehicle: NonZeroUsize, vehicle_body: u32) -> bool {
    (unsafe { owner_of(vehicle_body) }) == Some(vehicle)
}

/// The vehicle's native rigid body, verified against its current owner.
///
/// # Safety
///
/// The game thread, outside the physics step; `vehicle` must be a live vehicle.
#[inline]
pub unsafe fn vehicle_body(vehicle: NonZeroUsize) -> Option<u32> {
    let vehicle_ref = unsafe { &*(vehicle.get() as *const Vehicle) };
    let pfx = unsafe { vehicle_ref.m_PfxVehicle.cast::<PfxRigidBody>().as_ref() }?;
    let body = pfx.m_BodyId;
    unsafe { owns_body(vehicle, body) }.then_some(body)
}
