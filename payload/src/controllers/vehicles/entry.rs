//! One retained observation of the local character's vehicle for fitting and control ownership.

use jc3gi::{character::character::Character, vehicle::vehicle::Vehicle};

use crate::controllers::weapons::hold::refs::Strong;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct VehicleEntryIdentity {
    vehicle: usize,
    owner: usize,
}

pub(super) struct VehicleEntry {
    pub(super) identity: VehicleEntryIdentity,
    owner: Strong,
}

impl VehicleEntry {
    /// Capture the actual vehicle and keep it stable through this input tick.
    ///
    /// # Safety
    ///
    /// The caller runs on the main thread outside the world update.
    pub(super) unsafe fn read() -> Option<Self> {
        let character = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
        let owner = unsafe {
            Strong::filled(|out| {
                character.GetVehicle(out.cast());
            })
        }?;
        let identity = VehicleEntryIdentity {
            vehicle: owner.object() as usize,
            owner: owner.control_block() as usize,
        };
        Some(Self { identity, owner })
    }

    pub(super) fn into_owner(self) -> Strong {
        self.owner
    }

    pub(super) fn vehicle(&self) -> &Vehicle {
        // SAFETY: GetVehicle supplies the typed vehicle and the retained owner keeps it alive.
        unsafe { &*self.owner.object().cast::<Vehicle>() }
    }
}

#[cfg(test)]
impl VehicleEntryIdentity {
    pub(super) fn fixture(vehicle: usize, owner: usize) -> Self {
        Self { vehicle, owner }
    }
}
