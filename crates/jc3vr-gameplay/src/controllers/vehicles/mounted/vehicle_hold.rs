//! A vehicle's hold on the mounted gun it carries, and its release for a tear.
//!
//! A vehicle keeps a strong reference to each mounted gun it carries
//! ([`Vehicle::m_MountedWeapon`]) and places every one on its mount part each update
//! ([`Vehicle::UpdatePostParts_WeaponsAttachment_Parallel`]). The game lets a vehicle's gun go only
//! when the vehicle explodes: it drops the gun ([`WeaponBase::OnDrop`]) and then clears its entry,
//! so the vehicle stops placing it and it falls free. A tear does the same for the one gun, after
//! the gun's own detach ([`WeaponBase::DetachMountedGun`]) has stopped its use and dropped it.
//!
//! [`WeaponBase::OnDrop`]: jc3gi::equipment::weapon::WeaponBase::OnDrop
//! [`WeaponBase::DetachMountedGun`]: jc3gi::equipment::weapon::WeaponBase::DetachMountedGun

use std::num::NonZeroUsize;

use jc3gi::{
    equipment::weapon::WeaponBase, types::shared_ptr::SharedPtr, vehicle::vehicle::Vehicle,
};
use jc3vr_body::controllers::physics::{
    bodies::physics_object_of,
    refs::{Strong, Weak},
};

use crate::controllers::physical_hands::doors::engine::vehicle_of;

/// The vehicle that carries a mounted gun.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct VehicleHold {
    vehicle: NonZeroUsize,
}

/// Why a vehicle's hold could not be released.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum HoldError {
    /// The vehicle is in a parallel update phase, during which the game defers its own changes.
    Busy,
    /// The vehicle no longer lists the gun among its mounted weapons.
    NotListed,
}

impl HoldError {
    pub(super) const fn label(self) -> &'static str {
        match self {
            HoldError::Busy => "the vehicle is in a parallel update",
            HoldError::NotListed => "the vehicle does not list the gun",
        }
    }
}

impl VehicleHold {
    /// The vehicle holding `weapon`, when its holder is a vehicle that lists it.
    ///
    /// # Safety
    ///
    /// The game thread; `weapon` is live.
    pub(super) unsafe fn of(weapon: &WeaponBase) -> Option<Self> {
        if weapon.m_Holder.pn.is_null() {
            return None;
        }
        let holder = unsafe { Weak::of(&weapon.m_Holder) }.lock()?;
        let object = unsafe { physics_object_of(NonZeroUsize::new(holder.object() as usize)?) }?;
        let vehicle = unsafe { vehicle_of(object) }?;
        let hold = Self { vehicle };
        unsafe { hold.entry(weapon) }.is_some().then_some(hold)
    }

    /// Whether the vehicle can be changed now: outside its parallel update phases.
    ///
    /// # Safety
    ///
    /// The game thread; the vehicle is live.
    pub(super) unsafe fn ready(self) -> bool {
        unsafe { (*self.vehicle()).m_InParallelUpdatePhaseCounter <= 0 }
    }

    /// Clear the vehicle's entry for `weapon`, releasing its reference, so it no longer places the
    /// gun, as the vehicle's explosion does. Call it after the gun is dropped, so the gun's other
    /// owners keep it alive.
    ///
    /// # Safety
    ///
    /// The game thread, outside the world update; the vehicle and `weapon` are live.
    pub(super) unsafe fn release(self, weapon: &WeaponBase) -> Result<(), HoldError> {
        if !unsafe { self.ready() } {
            return Err(HoldError::Busy);
        }
        let entry = unsafe { self.entry(weapon) }.ok_or(HoldError::NotListed)?;
        let entry = unsafe { &mut *entry };
        let taken = SharedPtr {
            px: entry.px.cast(),
            pn: entry.pn,
        };
        entry.px = std::ptr::null_mut();
        entry.pn = std::ptr::null_mut();
        // SAFETY: the entry's reference is now the mod's to release; a shared pointer's release
        // reads only its control block.
        drop(unsafe { Strong::adopt(taken) });
        Ok(())
    }

    fn vehicle(self) -> *const Vehicle {
        self.vehicle.get() as *const Vehicle
    }

    /// The vehicle's mounted-weapon entry for `weapon`.
    ///
    /// # Safety
    ///
    /// The game thread; the vehicle is live.
    unsafe fn entry(self, weapon: &WeaponBase) -> Option<*mut SharedPtr<WeaponBase>> {
        let list = unsafe { &(*self.vehicle()).m_MountedWeapon };
        let (begin, end) = (list.begin, list.end);
        if begin.is_null() || end < begin {
            return None;
        }
        let count = unsafe { end.offset_from(begin) } as usize;
        (0..count)
            .map(|i| unsafe { begin.add(i) })
            .find(|&entry| std::ptr::eq(unsafe { (*entry).px }, weapon))
    }
}
