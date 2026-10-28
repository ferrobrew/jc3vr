//! Revalidate a queued character grip before reading a character or its current vehicle.

use std::num::NonZeroUsize;

use glam::{Mat4, Vec3};
use jc3gi::character::character::Character;

use crate::controllers::physical_hands::{
    bodies::owner_of,
    characters::{CharacterTarget, CharactersConfig, Seated, Seize, frame, open_door_near},
};

/// # Safety
/// The game thread, outside the physics step. The queued target need not still be live.
pub(super) unsafe fn take(
    target: CharacterTarget,
    point: Vec3,
    palm: Vec3,
    world_scale: f32,
    config: &CharactersConfig,
) -> Option<Seize> {
    take_with(
        target,
        point,
        Conditions {
            palm,
            world_scale,
            config,
        },
        &mut NativeAcquisition,
    )
}

/// # Safety
/// The game thread, outside the physics step. The saved vehicle pointer is never dereferenced.
pub(super) unsafe fn seated(
    target: CharacterTarget,
    palm: Vec3,
    world_scale: f32,
    config: &CharactersConfig,
) -> Option<Seated> {
    refresh(
        target,
        Conditions {
            palm,
            world_scale,
            config,
        },
        &mut NativeAcquisition,
    )?
    .seated
}

fn take_with(
    target: CharacterTarget,
    point: Vec3,
    conditions: Conditions<'_>,
    engine: &mut impl AcquisitionEngine,
) -> Option<Seize> {
    let target = refresh(target, conditions, engine)?;
    let grab = engine
        .frame(target.character)
        .inverse()
        .transform_point3(point);
    grab.is_finite().then_some(Seize { target, grab })
}

fn refresh(
    mut target: CharacterTarget,
    conditions: Conditions<'_>,
    engine: &mut impl AcquisitionEngine,
) -> Option<CharacterTarget> {
    if target.ragdoll || engine.owner(target.body) != Some(target.character) {
        return None;
    }
    // Ownership proves the character live before the adapter forms any character reference.
    target.seated = match (target.seated, engine.attachment(target.character)?) {
        (None, Attachment::OnFoot) if conditions.config.grab => None,
        (Some(before), Attachment::Vehicle(vehicle))
            if conditions.config.hijack && before.vehicle == vehicle =>
        {
            // The current character supplies this vehicle, not the queued seat observation.
            Some(engine.open_door(vehicle, conditions)?)
        }
        _ => return None,
    };
    Some(target)
}

#[derive(Copy, Clone)]
struct Conditions<'a> {
    palm: Vec3,
    world_scale: f32,
    config: &'a CharactersConfig,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Attachment {
    OnFoot,
    Vehicle(NonZeroUsize),
}

trait AcquisitionEngine {
    fn owner(&mut self, body: u32) -> Option<NonZeroUsize>;
    fn attachment(&mut self, character: NonZeroUsize) -> Option<Attachment>;
    fn open_door(&mut self, vehicle: NonZeroUsize, conditions: Conditions<'_>) -> Option<Seated>;
    fn frame(&mut self, character: NonZeroUsize) -> Mat4;
}

struct NativeAcquisition;

impl AcquisitionEngine for NativeAcquisition {
    fn owner(&mut self, body: u32) -> Option<NonZeroUsize> {
        unsafe { owner_of(body) }
    }

    fn attachment(&mut self, character: NonZeroUsize) -> Option<Attachment> {
        // SAFETY: refresh checks the body's owner before calling this method.
        let character = unsafe { &*(character.get() as *const Character) };
        if !unsafe { character.IsAttachedToVehicle() } {
            return Some(Attachment::OnFoot);
        }
        let vehicle = NonZeroUsize::new(unsafe { character.GetVehiclePtr() } as usize)?;
        Some(Attachment::Vehicle(vehicle))
    }

    fn open_door(&mut self, vehicle: NonZeroUsize, conditions: Conditions<'_>) -> Option<Seated> {
        // SAFETY: refresh obtains the vehicle from the owner-checked live character this call.
        let (door, open_deg) = unsafe {
            open_door_near(
                vehicle,
                conditions.palm,
                conditions.world_scale,
                conditions.config,
            )
        }?;
        Some(Seated {
            vehicle,
            door,
            open_deg,
        })
    }

    fn frame(&mut self, character: NonZeroUsize) -> Mat4 {
        // SAFETY: refresh checks body ownership and current acquisition eligibility first.
        unsafe { frame(character) }
    }
}

#[cfg(test)]
mod tests;
