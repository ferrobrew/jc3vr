//! The vehicle doors' engine side: a vehicle's door controller, each door's part and box, and the
//! latch (`docs/engine/gameplay/vehicle-doors.md`).
//!
//! A vehicle's doors are the [`DoorInfo`]s of its door controller
//! ([`VehicleDoorController`](jc3gi::vehicle::vehicle::VehicleDoorController)), kept in a
//! red-black tree keyed by [`DoorSlot`]. A closed door is merged into the vehicle's body; an open
//! one hangs on its hinge as a body of its own ([`PartInstance::GetPfxVehiclePartIfAny`]) owned by
//! a [`VehiclePartGameObject`]. The latch is undone the way the door controller undoes it, without
//! the hinge's motor: [`Vehicle::OpenDoor`] puts the vehicle's body in the world and the vehicle in
//! the update, so that the controller's own latch runs on the door afterwards, and its opening
//! request is withdrawn before the controller acts on it;
//! [`PfxBreakable::EnablePart`](jc3gi::vehicle::parts::PfxBreakable::EnablePart) then loosens the
//! part onto its hinge.
//!
//! Every function here runs on the game thread outside the physics step.

use std::{ffi::CStr, fmt, num::NonZeroUsize, ptr};

use glam::{Mat4, Vec3};
use jc3gi::{
    types::math::Vector3,
    vehicle::{
        parts::{PartEventReason, PartInstance, VehiclePartGameObject},
        vehicle::{DoorInfo, DoorMapNode, DoorSlot, DoorState, Vehicle},
    },
};

use crate::controllers::physical_hands::{
    bodies::{is_type, owner_of},
    doors::geometry::DoorBox,
};

/// Why a door could not be worked.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DoorError {
    /// The vehicle is gone: its body no longer belongs to it.
    VehicleGone,
    /// The vehicle has no physics body or no such part.
    NoPart,
    /// The part broke off, or has no door entry.
    NoDoor,
    /// The part did not come loose onto its hinge.
    StayedLatched,
}

impl fmt::Display for DoorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            DoorError::VehicleGone => "the vehicle is gone",
            DoorError::NoPart => "the vehicle has no such part",
            DoorError::NoDoor => "the part is no door, or broke off",
            DoorError::StayedLatched => "the door did not come loose onto its hinge",
        };
        write!(f, "physical hands: doors: {what}")
    }
}

impl std::error::Error for DoorError {}

/// One of a vehicle's doors as a hand sees it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DoorRead {
    pub slot: DoorSlot,
    pub part_index: u32,
    /// Whether the door is merged into the vehicle's body rather than hanging on its hinge.
    pub latched: bool,
    /// The door's box, from the part's root-space transform; `None` for a part without a model.
    pub door_box: Option<DoorBox>,
    /// The door's own body and its owner, while it hangs on its hinge.
    pub body: Option<(NonZeroUsize, u32)>,
    /// The door part's name hash, from its entry in the vehicle's parts resource; `None` for a
    /// part without one.
    pub part_hash: Option<u32>,
}

/// The vehicle a ray hit on `object` belongs to: the vehicle itself, or the vehicle a loosened
/// part such as an open door came from.
///
/// # Safety
///
/// `object` must be a live physics game object, from a cast this tick.
pub(crate) unsafe fn vehicle_of(object: NonZeroUsize) -> Option<NonZeroUsize> {
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
pub(crate) unsafe fn is_vehicle_part(object: NonZeroUsize) -> bool {
    unsafe { is_type(object, VehiclePartGameObject::TYPE_ID()) }
}

/// The vehicle's world transform at the end of the current simulation step.
///
/// # Safety
///
/// `vehicle` must be a live vehicle.
pub(crate) unsafe fn vehicle_frame(vehicle: NonZeroUsize) -> Mat4 {
    let vehicle = unsafe { &*(vehicle.get() as *const Vehicle) };
    Mat4::from(vehicle.m_WorldMatrixT1)
}

/// Every door of `vehicle` that has not broken off.
///
/// # Safety
///
/// `vehicle` must be a live vehicle.
pub(crate) unsafe fn doors(vehicle: NonZeroUsize) -> Vec<DoorRead> {
    let world = unsafe { vehicle_frame(vehicle) };
    let mut doors = Vec::new();
    unsafe {
        each_door(vehicle, |_, slot, part| {
            doors.push(DoorRead {
                slot,
                part_index: part.m_PartInstanceIndex,
                latched: !part.IsConstraintActive(),
                door_box: door_box(world, part),
                body: loose_body(part),
                part_hash: part.m_PartProperties.as_ref().map(|p| p.m_name.m_hash),
            });
        })
    };
    doors
}

/// Whether `vehicle` still owns its body `vehicle_body`, which proves it is alive.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn alive(vehicle: NonZeroUsize, vehicle_body: u32) -> bool {
    (unsafe { owner_of(vehicle_body) }) == Some(vehicle)
}

/// The door of `vehicle` at part index `part_index`, read now.
///
/// # Safety
///
/// `vehicle` must be a live vehicle.
pub(crate) unsafe fn door(vehicle: NonZeroUsize, part_index: u32) -> Option<DoorRead> {
    unsafe { doors(vehicle) }
        .into_iter()
        .find(|d| d.part_index == part_index)
}

/// Undo the latch of the door at part index `part_index` of `vehicle`, and return the door's own
/// body and its owner, now hanging on its hinge.
///
/// # Safety
///
/// The game thread, outside the physics step; `vehicle` must be a live vehicle.
pub(crate) unsafe fn unlatch(
    vehicle: NonZeroUsize,
    part_index: u32,
) -> Result<(NonZeroUsize, u32), DoorError> {
    let read = unsafe { door(vehicle, part_index) }.ok_or(DoorError::NoDoor)?;
    let vehicle_ref = unsafe { &mut *(vehicle.get() as *mut Vehicle) };
    let pfx = unsafe { vehicle_ref.m_PfxVehicle.as_mut() }.ok_or(DoorError::NoPart)?;
    let part = unsafe { pfx.GetPartByIndex(part_index) };
    if part.is_null() {
        return Err(DoorError::NoPart);
    }
    if read.latched {
        // The vehicle's body joins the world and the vehicle the update, as for any opening, so
        // that the door controller's own latch acts on the loose door; the motor that request would
        // start is withdrawn before the controller's next update reads it.
        unsafe { vehicle_ref.OpenDoor(read.slot, false, 0.0) };
        unsafe {
            each_door(vehicle, |info, slot, _| {
                if slot == read.slot
                    && state_of(ptr::addr_of!(info.m_PendingState))
                        == DoorState::DOOR_OPENING as i32
                {
                    info.m_PendingState = DoorState::DOOR_CLOSED;
                }
            })
        };
        unsafe { pfx.EnablePart(part, PartEventReason::EPER_PartOperated, true) };
    }
    let part_ref = unsafe { &mut *part };
    if !unsafe { part_ref.IsConstraintActive() } {
        return Err(DoorError::StayedLatched);
    }
    let body = unsafe { loose_body(part_ref) }.ok_or(DoorError::StayedLatched)?;
    // The state the controller gives a door whose part it spawns, as an opened door's.
    unsafe {
        each_door(vehicle, |info, _, p| {
            if p.m_PartInstanceIndex == part_index {
                info.m_State = DoorState::DOOR_OPEN;
            }
        })
    };
    Ok(body)
}

/// The angle the door at part index `part_index` of `vehicle` stands open, radians: its hinge's
/// angle above the hinge's smallest. Zero for a latched door.
///
/// # Safety
///
/// `vehicle` must be a live vehicle.
pub(crate) unsafe fn open_angle(vehicle: NonZeroUsize, part_index: u32) -> f32 {
    let vehicle = unsafe { &mut *(vehicle.get() as *mut Vehicle) };
    let Some(pfx) = (unsafe { vehicle.m_PfxVehicle.as_mut() }) else {
        return 0.0;
    };
    let Ok(index) = i32::try_from(part_index) else {
        return 0.0;
    };
    let angle = unsafe { pfx.GetHingeAngleAboveMin(index) };
    if angle.is_finite() { angle.abs() } else { 0.0 }
}

/// The name of the door part at part index `part_index` of `vehicle`, from the vehicle's parts
/// resource; `None` for a part without one.
///
/// # Safety
///
/// `vehicle` must be a live vehicle.
pub(crate) unsafe fn part_name(vehicle: NonZeroUsize, part_index: u32) -> Option<String> {
    let vehicle = unsafe { &mut *(vehicle.get() as *mut Vehicle) };
    let pfx = unsafe { vehicle.m_PfxVehicle.as_mut() }?;
    let part = unsafe { pfx.GetPartByIndex(part_index).as_ref() }?;
    let properties = unsafe { part.m_PartProperties.as_ref() }?;
    let name = properties.m_name.m_string;
    if name.is_null() {
        return None;
    }
    // SAFETY: the resource's NUL-terminated name, resolved when the resource was loaded.
    Some(
        unsafe { CStr::from_ptr(name.cast()) }
            .to_string_lossy()
            .into_owned(),
    )
}

/// The most doors a vehicle's door map is walked for, against a corrupt tree.
const MAX_DOORS: usize = 64;

/// Call `f` with each door of `vehicle`, its slot, and its part, for the doors of a known slot whose
/// part has not broken off.
///
/// # Safety
///
/// `vehicle` must be a live vehicle.
unsafe fn each_door(
    vehicle: NonZeroUsize,
    mut f: impl FnMut(&mut DoorInfo, DoorSlot, &mut PartInstance),
) {
    let vehicle = unsafe { &mut *(vehicle.get() as *mut Vehicle) };
    let head = vehicle.m_VehicleDoorController.m_DoorMapHead;
    let Some(head_ref) = (unsafe { head.as_ref() }) else {
        return;
    };
    let mut stack = vec![head_ref.m_Parent];
    let mut visited = 0;
    while let Some(node) = stack.pop() {
        if node.is_null() || node == head || visited >= MAX_DOORS {
            continue;
        }
        let node: &DoorMapNode = unsafe { &*node };
        if node.m_IsNil {
            continue;
        }
        visited += 1;
        stack.push(node.m_Left);
        stack.push(node.m_Right);
        let (begin, end) = (node.m_DoorsBegin, node.m_DoorsEnd);
        if begin.is_null() || end < begin {
            continue;
        }
        let count = (end as usize - begin as usize) / size_of::<DoorInfo>();
        for k in 0..count.min(MAX_DOORS) {
            let info = unsafe { &mut *begin.add(k) };
            if state_of(ptr::addr_of!(info.m_State)) == DoorState::DOOR_MISSING as i32 {
                continue;
            }
            let Some(slot) = slot_of(info) else {
                continue;
            };
            let Some(part) = (unsafe { info.m_PartInstance.as_mut() }) else {
                continue;
            };
            if unsafe { part.BrokeOffOrAncestorBrokeOff() } {
                continue;
            }
            f(info, slot, part);
        }
    }
}

/// A door state's raw value, read without assuming the engine wrote a known one.
fn state_of(state: *const DoorState) -> i32 {
    // SAFETY: the field of a live door entry, read as the integer it is stored as.
    unsafe { state.cast::<i32>().read() }
}

/// The door's slot, read as its raw value; `None` for a value that is no known slot.
fn slot_of(info: &DoorInfo) -> Option<DoorSlot> {
    // SAFETY: the field of a live door entry, read as the integer it is stored as.
    let raw = unsafe { ptr::addr_of!(info.m_DoorSlot).cast::<i32>().read() };
    DOOR_SLOTS.into_iter().find(|s| *s as i32 == raw)
}

const DOOR_SLOTS: [DoorSlot; 13] = [
    DoorSlot::FRONT_LEFT,
    DoorSlot::FRONT_RIGHT,
    DoorSlot::REAR_LEFT,
    DoorSlot::REAR_RIGHT,
    DoorSlot::CENTER_LEFT,
    DoorSlot::CENTER_RIGHT,
    DoorSlot::TURRET_DOOR,
    DoorSlot::TRUNK,
    DoorSlot::HOOD,
    DoorSlot::REAR_DOOR,
    DoorSlot::CARGO_BAY_DOOR,
    DoorSlot::FOLDING_WING,
    DoorSlot::MISSILE_HATCH,
];

/// The part's box in the world on a vehicle at `world`.
///
/// # Safety
///
/// `part` must be a live part instance.
unsafe fn door_box(world: Mat4, part: &PartInstance) -> Option<DoorBox> {
    let model = unsafe { part.m_PartModelInstance.as_ref() }?;
    let mut min = Vector3 { data: [0.0; 3] };
    let mut max = Vector3 { data: [0.0; 3] };
    unsafe { model.GetModelSpaceBoundingBox(&mut min, &mut max) };
    DoorBox::new(
        world,
        Mat4::from(part.m_RootSpaceTransform),
        Vec3::from_array(min.data),
        Vec3::from_array(max.data),
    )
}

/// The loosened part's own body and the object that owns it.
///
/// # Safety
///
/// `part` must be a live part instance, on the game thread outside the physics step.
unsafe fn loose_body(part: &mut PartInstance) -> Option<(NonZeroUsize, u32)> {
    let body = unsafe { part.GetPfxVehiclePartIfAny().as_ref() }?.m_BodyId;
    let owner = unsafe { owner_of(body) }?;
    Some((owner, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_slot_is_listed_once() {
        for (k, slot) in DOOR_SLOTS.iter().enumerate() {
            assert_eq!(*slot as i32, k as i32);
        }
    }
}
