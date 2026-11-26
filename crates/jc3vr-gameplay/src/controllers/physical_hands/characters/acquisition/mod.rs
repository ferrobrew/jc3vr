//! Revalidate a queued character grip before reading a character or its current vehicle.

use std::num::NonZeroUsize;

use glam::{Mat4, Vec3};
use jc3gi::character::character::{Character, SafeBoneIndex};
use jc3vr_body::controllers::{
    grab::world_reach::{CharacterTarget, SeatAccess, Seated},
    physics::{bodies::owner_of, characters as engine},
};

use crate::controllers::physical_hands::characters::{
    CharactersConfig, Seize, bone_frame, frame, open_door_near,
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
    let (bone, matrix) = engine.anchor(target.character, point);
    if !point.is_finite() || !matrix.is_finite() || matrix.determinant().abs() <= 1e-6 {
        return None;
    }
    let grab = matrix.inverse().transform_point3(point);
    grab.is_finite().then_some(Seize {
        target,
        grab,
        bone,
        in_palm: Vec3::ZERO,
    })
}

fn refresh(
    mut target: CharacterTarget,
    conditions: Conditions<'_>,
    engine: &mut impl AcquisitionEngine,
) -> Option<CharacterTarget> {
    if engine.owner(target.body) != Some(target.character) {
        return None;
    }
    // A ragdolling character lies free of any seat, and the body the reach took is one of its
    // ragdoll's, so ownership alone admits it: the hand holds the bone, and the rest of the
    // ragdoll hangs from it by the ragdoll's own constraints.
    if target.ragdoll {
        if !conditions.config.grab {
            return None;
        }
        target.seated = None;
        return Some(target);
    }
    // Ownership proves the character live before the adapter forms any character reference.
    target.seated = match (target.seated, engine.attachment(target.character)?) {
        (None, Attachment::OnFoot) if conditions.config.grab => None,
        (Some(before), Attachment::Vehicle(vehicle))
            if conditions.config.hijack && before.vehicle == vehicle =>
        {
            match before.access {
                // The current character supplies this vehicle, not the queued seat observation.
                SeatAccess::Door { .. } => Some(engine.open_door(vehicle, conditions)?),
                // A window the glass brought down stays down: the reach that found the way in
                // judged it, and the hand, now inside the cabin, keeps it while the character
                // keeps the seat.
                SeatAccess::Pane => Some(Seated {
                    vehicle,
                    access: SeatAccess::Pane,
                }),
                SeatAccess::Exposed => return None,
            }
        }
        (Some(before), Attachment::Exposed(vehicle))
            if conditions.config.hijack
                && before.vehicle == vehicle
                && before.access == SeatAccess::Exposed =>
        {
            Some(Seated {
                vehicle,
                access: SeatAccess::Exposed,
            })
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
    Exposed(NonZeroUsize),
}

trait AcquisitionEngine {
    fn owner(&mut self, body: u32) -> Option<NonZeroUsize>;
    fn attachment(&mut self, character: NonZeroUsize) -> Option<Attachment>;
    fn open_door(&mut self, vehicle: NonZeroUsize, conditions: Conditions<'_>) -> Option<Seated>;
    fn frame(&mut self, character: NonZeroUsize) -> Mat4;
    fn anchor(&mut self, character: NonZeroUsize, _point: Vec3) -> (Option<SafeBoneIndex>, Mat4) {
        (None, self.frame(character))
    }
}

struct NativeAcquisition;

impl AcquisitionEngine for NativeAcquisition {
    fn anchor(&mut self, character: NonZeroUsize, point: Vec3) -> (Option<SafeBoneIndex>, Mat4) {
        let live = unsafe { &*(character.get() as *const Character) };
        [
            SafeBoneIndex::HIPS,
            SafeBoneIndex::SPINE,
            SafeBoneIndex::SPINE1,
            SafeBoneIndex::SPINE2,
            SafeBoneIndex::NECK,
            SafeBoneIndex::HEAD,
            SafeBoneIndex::LEFT_SHOULDER,
            SafeBoneIndex::RIGHT_SHOULDER,
            SafeBoneIndex::LEFT_ARM,
            SafeBoneIndex::RIGHT_ARM,
            SafeBoneIndex::LEFT_HAND,
            SafeBoneIndex::RIGHT_HAND,
            SafeBoneIndex::LEFT_UP_LEG,
            SafeBoneIndex::RIGHT_UP_LEG,
            SafeBoneIndex::LEFT_LEG,
            SafeBoneIndex::RIGHT_LEG,
            SafeBoneIndex::LEFT_FOOT,
            SafeBoneIndex::RIGHT_FOOT,
        ]
        .into_iter()
        .filter_map(|bone| unsafe { bone_frame(live, bone) }.map(|matrix| (bone, matrix)))
        .min_by(|a, b| {
            a.1.w_axis
                .truncate()
                .distance_squared(point)
                .total_cmp(&b.1.w_axis.truncate().distance_squared(point))
        })
        .map_or_else(
            || (None, self.frame(character)),
            |(bone, matrix)| (Some(bone), matrix),
        )
    }

    fn owner(&mut self, body: u32) -> Option<NonZeroUsize> {
        unsafe { owner_of(body) }
    }

    fn attachment(&mut self, character: NonZeroUsize) -> Option<Attachment> {
        // SAFETY: refresh checks the body's owner before calling this method.
        let character = unsafe { &*(character.get() as *const Character) };
        if unsafe { engine::protection(character) }.is_some()
            || unsafe { engine::dead(character) }
            || unsafe { character.IsRagdolling() }
        {
            return None;
        }
        if !unsafe { character.IsAttachedToVehicle() } {
            return Some(Attachment::OnFoot);
        }
        let vehicle = NonZeroUsize::new(unsafe { character.GetVehiclePtr() } as usize)?;
        Some(if unsafe { character.IsRidingMC() } {
            Attachment::Exposed(vehicle)
        } else {
            Attachment::Vehicle(vehicle)
        })
    }

    fn open_door(&mut self, vehicle: NonZeroUsize, conditions: Conditions<'_>) -> Option<Seated> {
        // SAFETY: refresh obtains the vehicle from the owner-checked live character this call.
        let (slot, open_deg) =
            unsafe { open_door_near(vehicle, conditions.palm, conditions.world_scale) }?;
        Some(Seated {
            vehicle,
            access: SeatAccess::Door { slot, open_deg },
        })
    }

    fn frame(&mut self, character: NonZeroUsize) -> Mat4 {
        // SAFETY: refresh checks body ownership and current acquisition eligibility first.
        unsafe { frame(character) }
    }
}

#[cfg(test)]
mod tests;
