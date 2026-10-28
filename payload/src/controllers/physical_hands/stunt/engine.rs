//! The stunt positions' engine side: which stunt position the game has the player in, the
//! vehicle a hand grips, its stunt areas, and the entry into a stunt position
//! (`docs/engine/gameplay/stunt-positions.md`).
//!
//! The game holds a character in a stunt position through the vehicle's interaction graph: a
//! standing character occupies a stunt-area node ([`Character::IsInStuntArea`]), and a character
//! on a helicopter hangs from it ([`Character::IsStuntHanging`]). The entry is the game's own
//! ([`Character::AutoAttachToClosestStuntAreaOnVehicle`], and for a character standing on a
//! vehicle [`Character::AutoAttachToStuntAreaOnVehicleBelow`]), which the game's graph scripts and
//! on-foot movement tasks call from the game update.
//!
//! Every function here runs on the game thread outside the world update (the input tick): the
//! predicates walk the character's rule system and interaction graph, and the entry changes them.

use std::{fmt, num::NonZeroUsize, ptr};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    types::math::Vector3,
    vehicle::{
        stunt_areas::GetNearestStuntArea_World,
        vehicle::{Helicopter, Vehicle},
    },
};

use crate::{
    controllers::physical_hands::{
        bodies::{is_type, owner_of},
        stunt::solve::StuntKind,
    },
    sim_rate,
};

/// Why the game did not take the player into a stunt position.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StuntError {
    /// There is no local player character.
    NoPlayer,
    /// The game refused: a helicopter without a hang, or a vehicle without a stunt area or the
    /// interaction that enters one.
    Refused,
}

impl fmt::Display for StuntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            StuntError::NoPlayer => "there is no local player character",
            StuntError::Refused => "the game refused the stunt position",
        };
        write!(f, "stunt positions: {what}")
    }
}

impl std::error::Error for StuntError {}

/// The stunt position the game has the local player in, and the speed of the vehicle it is on.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PositionRead {
    pub kind: Option<StuntKind>,
    /// The vehicle's speed over the last simulation step, metres per second; zero off a vehicle.
    pub vehicle_speed_mps: f32,
}

/// Read the local player's stunt position, or `None` without a player.
///
/// # Safety
///
/// The game thread, outside the world update.
pub(super) unsafe fn position() -> Option<PositionRead> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    let kind = unsafe {
        if !character.IsInStuntPosition() {
            None
        } else if character.IsStuntHanging() {
            Some(StuntKind::Hanging)
        } else if character.IsInStuntArea() && !character.IsStuntTraversing() {
            Some(StuntKind::Standing)
        } else {
            None
        }
    };
    let vehicle_speed_mps = kind
        .and_then(|_| unsafe { character.GetVehiclePtr().as_ref() })
        .map_or(0.0, |vehicle| {
            let from = Mat4::from(vehicle.m_WorldMatrixT0).w_axis.truncate();
            let to = Mat4::from(vehicle.m_WorldMatrixT1).w_axis.truncate();
            // The two transforms are a tick apart, over which the world takes one step.
            let speed = from.distance(to) / sim_rate::world_step_seconds(None);
            if speed.is_finite() { speed } else { 0.0 }
        });
    Some(PositionRead {
        kind,
        vehicle_speed_mps,
    })
}

/// The vehicle that owns the body `body`, if a vehicle does.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(super) unsafe fn vehicle_of(body: u32) -> Option<NonZeroUsize> {
    let owner = unsafe { owner_of(body) }?;
    unsafe { is_type(owner, Vehicle::TYPE_ID()) }.then_some(owner)
}

/// Whether the live vehicle `vehicle` is a helicopter.
///
/// # Safety
///
/// `vehicle` must be a live vehicle.
pub(super) unsafe fn is_helicopter(vehicle: NonZeroUsize) -> bool {
    unsafe { is_type(vehicle, Helicopter::TYPE_ID()) }
}

/// How far `at` is from the nearest point of `vehicle`'s stunt areas, metres, or `None` for a
/// vehicle without one ([`GetNearestStuntArea_World`]).
///
/// # Safety
///
/// `vehicle` must be a live vehicle, on the game thread outside the world update.
pub(super) unsafe fn stunt_area_distance(vehicle: NonZeroUsize, at: Vec3) -> Option<f32> {
    let vehicle = vehicle.get() as *const Vehicle;
    let position = Vector3::from(at);
    let mut closest = Vector3::from(Vec3::ZERO);
    let mut distance_squared = f32::MAX;
    let mut found = false;
    let area = unsafe {
        GetNearestStuntArea_World(
            vehicle,
            ptr::addr_of!((*vehicle).m_StuntAreas),
            &position,
            &mut closest,
            &mut distance_squared,
            &mut found,
            false,
        )
    };
    if area.is_null() || !found {
        return None;
    }
    let distance = Vec3::from(closest).distance(at);
    distance.is_finite().then_some(distance)
}

/// How the game took the player into a stunt position.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Entry {
    /// Onto the stunt area under the player, where the player stands.
    Below,
    /// Onto the stunt position nearest the point given, turned to the vehicle's heading.
    Nearest,
}

impl Entry {
    pub const fn label(self) -> &'static str {
        match self {
            Entry::Below => "the stunt area below",
            Entry::Nearest => "the nearest stunt position",
        }
    }
}

/// Take the local player into a roof stunt position on `vehicle` near `from`. The entry first
/// asks for the stunt area under the player, which keeps the player's heading, then tries the
/// nearest stunt position, which turns the player to the vehicle's heading.
///
/// # Safety
///
/// `vehicle` must be a live vehicle, on the game thread outside the world update.
pub(super) unsafe fn enter(vehicle: NonZeroUsize, from: Vec3) -> Result<Entry, StuntError> {
    let character =
        unsafe { Character::GetLocalPlayerCharacter().as_mut() }.ok_or(StuntError::NoPlayer)?;
    if !unsafe { character.IsThisVehicleBelow(vehicle.get() as *const Vehicle) } {
        return Err(StuntError::Refused);
    }
    if crate::controllers::physical_hands::climb::native::deliberate(character, || unsafe {
        character.AutoAttachToStuntAreaOnVehicleBelow()
    }) {
        return Ok(Entry::Below);
    }
    let position = Vector3::from(from);
    if unsafe {
        character.AutoAttachToClosestStuntAreaOnVehicle(vehicle.get() as *mut Vehicle, &position)
    } {
        Ok(Entry::Nearest)
    } else {
        Err(StuntError::Refused)
    }
}
