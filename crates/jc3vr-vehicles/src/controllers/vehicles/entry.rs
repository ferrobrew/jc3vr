//! One retained observation of the local character's vehicle for fitting and control ownership.

use jc3gi::vehicle::vehicle::Vehicle;
use jc3vr_body::controllers::physics::{local_player::LocalPlayer, refs::Strong};
use jc3vr_core::game_tick::GameTick;

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
    pub(super) fn read(game: &GameTick<'_>) -> Option<Self> {
        let local_player = LocalPlayer::get(game)?;
        let character = local_player.character();
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

    /// Whether the vehicle has nitrous: the game's effective nitrous upgrade level
    /// ([`Vehicle::GetCurrentEffectiveNitrousUpgradeLevel`]) is above none, as for the HUD's
    /// nitrous meter.
    pub(super) fn has_nitrous(&self) -> bool {
        // SAFETY: the retained owner keeps the vehicle alive; read on the game thread.
        unsafe { self.vehicle().GetCurrentEffectiveNitrousUpgradeLevel() > NO_NITROUS }
    }
}

/// The nitrous upgrade level of a vehicle without nitrous (`NVehicle::EVehicleModUpgradeLevel`).
const NO_NITROUS: i32 = -1;

#[cfg(test)]
impl VehicleEntryIdentity {
    pub(super) fn fixture(vehicle: usize, owner: usize) -> Self {
        Self { vehicle, owner }
    }
}
