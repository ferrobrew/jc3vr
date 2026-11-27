//! What a reached object is to a hand: a weapon lying in the world, a loose body, or something a
//! hand does not carry, and why. The body accessors this reads are in
//! [`jc3vr_body::controllers::physics::bodies`], re-exported here for the physical hands.

use std::{fmt, num::NonZeroUsize};

use jc3gi::{
    character::character::Character,
    equipment::{
        explosive::Grenade,
        weapon::{WeaponBase, WeaponState},
    },
    physics::RigidObject,
    vehicle::{parts::VehiclePartGameObject, vehicle::Vehicle},
};
use jc3vr_body::controllers::{
    aim::grapple_weapons,
    grab::world_reach::TargetKind,
    physics::{bodies::*, error::PhysicsError, vehicles},
};

use crate::controllers::physical_hands::doors::engine as door_engine;

/// What the object at `object`, reached on its body `body`, is to a hand: a weapon lying in the
/// world, or a finite-mass loose body, including a vehicle the player does not occupy. Parts still
/// attached to a vehicle remain in the door path.
///
/// # Safety
///
/// Main thread, outside the physics step; `object` and `body` must come from a cast this tick.
pub(super) unsafe fn classify(object: NonZeroUsize, body: u32) -> Result<TargetKind, Unclassified> {
    let game_object = game_object_of(object);
    if unsafe { fixed_fixture(object) } {
        return Err(Unclassified::Fixture);
    }
    if unsafe { (*game_object).IsType(WeaponBase::TYPE_ID()) } {
        let weapon = object.get() as *const WeaponBase;
        // Read as the raw value, so that a state the enum does not name is still reported.
        let state = unsafe { (&raw const (*weapon).m_State).cast::<i32>().read() };
        if state != WeaponState::E_WS_DROPPED as i32 {
            return Err(Unclassified::WeaponNotDropped(state));
        }
        if !unsafe { grapple_weapons::is_dropped_weapon(object) } {
            return Err(Unclassified::WeaponOwned);
        }
        unsafe { read(body, object) }.map_err(Unclassified::Body)?;
        return Ok(TargetKind::Weapon);
    }
    if unsafe { (*game_object).IsType(Vehicle::TYPE_ID()) } {
        let occupied =
            unsafe { Character::GetLocalPlayerCharacter().as_ref() }.is_some_and(|player| unsafe {
                player.IsAttachedToVehicle() && player.GetVehiclePtr() as usize == object.get()
            });
        return vehicle_kind(unsafe { read(body, object) }, occupied);
    }
    // A grenade belongs to the weapon handling, which takes a dropped one into the loadout and
    // catches a live one with its fuse running; held as a loose object it would be neither.
    if unsafe { (*game_object).IsType(Grenade::TYPE_ID()) } {
        return Err(Unclassified::Grenade);
    }
    // A part still on its vehicle, such as a door hanging on its hinge, is the doors' to take; one
    // that has come off, such as a door torn off, is a loose object like any other.
    if unsafe { vehicles::is_vehicle_part(object) }
        && !unsafe { door_engine::broken_off(object, body) }
    {
        return Err(Unclassified::VehiclePart);
    }
    match unsafe { read(body, object) } {
        Ok(state) => state
            .mass_kg
            .map(|mass_kg| TargetKind::Prop { mass_kg })
            .ok_or(Unclassified::NoMass),
        // A placed object at rest is as loose as a dynamic one: the game keeps its mass, and
        // [`loosen`] turns it dynamic when a hand or the snatch takes it.
        Err(PhysicsError::NotDynamic) => unsafe { placed_mass(object, body) }
            .map(|mass_kg| TargetKind::Prop { mass_kg })
            .ok_or(Unclassified::Body(PhysicsError::NotDynamic)),
        Err(e) => Err(Unclassified::Body(e)),
    }
}

fn vehicle_kind(
    state: Result<BodyState, PhysicsError>,
    occupied: bool,
) -> Result<TargetKind, Unclassified> {
    if occupied {
        return Err(Unclassified::VehicleMounted);
    }
    let mass_kg = state
        .map_err(Unclassified::Body)?
        .mass_kg
        .filter(|mass| mass.is_finite() && *mass > 0.0)
        .ok_or(Unclassified::NoMass)?;
    Ok(TargetKind::Prop { mass_kg })
}

/// Why a reached object is no loose object for a hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Unclassified {
    /// A weapon in this state (raw), not lying dropped in the world.
    WeaponNotDropped(i32),
    WeaponOwned,
    /// The reached body cannot be read as a loose one: static, keyframed, or gone.
    Body(PhysicsError),
    /// The body's mass is infinite.
    NoMass,
    /// A fixed gameplay fixture is neither carried nor activated.
    Fixture,
    /// The player occupies this vehicle.
    VehicleMounted,
    /// A part still on its vehicle.
    VehiclePart,
    /// A grenade, which the weapon handling takes.
    Grenade,
}

impl fmt::Display for Unclassified {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unclassified::WeaponNotDropped(state) => {
                write!(f, "a weapon that is not lying dropped (state {state})")
            }
            Unclassified::WeaponOwned => write!(f, "a weapon still held or used by an object"),
            Unclassified::Body(e) => write!(f, "{e}"),
            Unclassified::NoMass => write!(f, "its body's mass is infinite"),
            Unclassified::Fixture => write!(f, "a fixed gameplay fixture"),
            Unclassified::VehicleMounted => write!(f, "the player occupies this vehicle"),
            Unclassified::VehiclePart => write!(f, "a part still on its vehicle is never carried"),
            Unclassified::Grenade => write!(f, "a grenade is taken by the weapon handling"),
        }
    }
}

/// What kind of object `object` is, for the reach's log: the classes a hand tells apart, and the
/// address of its `CGameObject` subobject's virtual table, which names any other class in the
/// disassembly.
///
/// # Safety
///
/// `object` must be a live physics game object.
pub(super) unsafe fn describe(object: NonZeroUsize) -> String {
    let game_object = game_object_of(object);
    let is = |type_id: *const u32| unsafe { (*game_object).IsType(type_id) };
    let kind = if is(unsafe { WeaponBase::TYPE_ID() }) {
        "weapon"
    } else if is(unsafe { Vehicle::TYPE_ID() }) {
        "vehicle"
    } else if is(unsafe { VehiclePartGameObject::TYPE_ID() }) {
        "vehicle part"
    } else if is(unsafe { Character::TYPE_ID() }) {
        "character"
    } else if is(unsafe { RigidObject::TYPE_ID() }) {
        "rigid object"
    } else {
        "other object"
    };
    let vftable = unsafe { (*game_object).vftable() } as usize;
    format!("{kind} (vftable {vftable:#x})")
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec3};
    use jc3vr_body::controllers::physics::rigid::{BodyPose, Velocities};

    use super::*;

    #[test]
    fn a_vehicle_requires_dynamic_finite_mass_and_an_unoccupied_player_seat() {
        let state = |mass_kg| {
            Ok(BodyState {
                pose: BodyPose {
                    centre: Vec3::ZERO,
                    orientation: Quat::IDENTITY,
                },
                velocities: Velocities::default(),
                mass_kg,
                inverse_inertia: Vec3::ONE,
            })
        };
        assert_eq!(
            vehicle_kind(state(Some(200.0)), false),
            Ok(TargetKind::Prop { mass_kg: 200.0 })
        );
        assert_eq!(
            vehicle_kind(state(Some(200.0)), true),
            Err(Unclassified::VehicleMounted)
        );
        for mass in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f32::NAN),
            Some(f32::INFINITY),
        ] {
            assert_eq!(vehicle_kind(state(mass), false), Err(Unclassified::NoMass));
        }
        for why in [
            PhysicsError::NotDynamic,
            PhysicsError::NoBody,
            PhysicsError::Reowned,
        ] {
            assert_eq!(vehicle_kind(Err(why), false), Err(Unclassified::Body(why)));
        }
    }
}
