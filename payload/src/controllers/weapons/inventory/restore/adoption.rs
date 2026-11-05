//! Restored callback adoption uses one table admission for the physical and table destination.

use jc3gi::{character::character::Character, equipment::weapon::WeaponBase};

use crate::controllers::{
    loadout::{
        Restored, Socket, live,
        restore::{RestoreAdmission, RestoreRefusal},
    },
    weapons::{
        dual_wield::single,
        hold::refs::Strong,
        inventory::{
            carry::{self, CarryRefusal},
            engine::{self, kind_of, weapon_of},
            persist::Record,
            restore::{PLAN, SPAWNED, STATUS, expire_spawning},
        },
    },
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Restored(Socket),
    Refused(RestoreRefusal),
    CarryRefused(CarryRefusal),
}

pub(crate) trait Adapter {
    fn admit(&mut self, restored: Restored) -> Result<RestoreAdmission, RestoreRefusal>;
    fn carry(&mut self, admission: RestoreAdmission) -> Result<(), CarryRefusal>;
    fn commit(&mut self, admission: RestoreAdmission) -> Result<Socket, RestoreRefusal>;
    fn abandon(&mut self);
}

pub(crate) fn transfer(adapter: &mut impl Adapter, restored: Restored) -> Outcome {
    let admission = match adapter.admit(restored) {
        Ok(admission) => admission,
        Err(refusal) => return Outcome::Refused(refusal),
    };
    if let Err(refusal) = adapter.carry(admission) {
        return Outcome::CarryRefused(refusal);
    }
    match adapter.commit(admission) {
        Ok(socket) => Outcome::Restored(socket),
        Err(refusal) => {
            adapter.abandon();
            Outcome::Refused(refusal)
        }
    }
}

struct Native<'a> {
    owner: &'a mut Character,
    strong: Option<Strong>,
    carried: Option<usize>,
}

impl Adapter for Native<'_> {
    fn admit(&mut self, restored: Restored) -> Result<RestoreAdmission, RestoreRefusal> {
        live::admit_restore(restored)
    }

    fn carry(&mut self, admission: RestoreAdmission) -> Result<(), CarryRefusal> {
        if !live::restore_admission_current(admission) {
            return Err(CarryRefusal::NotDone);
        }
        self.carried = Some(unsafe {
            carry::carry_loose(
                self.owner,
                self.strong.take().unwrap(),
                admission.restored.socket,
            )?
        });
        Ok(())
    }

    fn commit(&mut self, admission: RestoreAdmission) -> Result<Socket, RestoreRefusal> {
        live::commit_restore(admission)
    }

    fn abandon(&mut self) {
        if let Some(object) = self.carried.take() {
            drop(carry::release(object));
        }
    }
}

/// Carry `strong` onto `restored`'s socket, or the table's next free socket that fits it, through
/// the table's restore admission. Returns the outcome and, when it was carried, the weapon's
/// address.
pub(super) fn carry_onto(
    owner: &mut Character,
    strong: Strong,
    restored: Restored,
) -> (Outcome, Option<usize>) {
    let mut native = Native {
        owner,
        strong: Some(strong),
        carried: None,
    };
    let outcome = transfer(&mut native, restored);
    let carried = matches!(outcome, Outcome::Restored(_))
        .then_some(native.carried)
        .flatten();
    (outcome, carried)
}

/// Adopt current-generation results only after the live table has completed its restoration.
pub(super) unsafe fn adopt(owner: &mut Character) {
    let generation = PLAN.lock().generation;
    let spawned = std::mem::take(&mut *SPAWNED.lock());
    for (asked_in, strong) in spawned {
        if asked_in != generation {
            continue;
        }
        // SAFETY: the callback's strong reference keeps the weapon alive.
        let weapon = unsafe { &mut *weapon_of(&strong) };
        let Some(kind) = (unsafe { kind_of(weapon) }) else {
            continue;
        };
        let record = {
            let mut plan = PLAN.lock();
            let index = plan
                .spawning
                .iter()
                .position(|s| s.record.tag_hash == kind.tag_hash);
            index.map(|i| plan.spawning.remove(i).record)
        };
        let Some(record) = record else {
            continue;
        };
        let requested = engine::object_address(weapon)
            .zip(unsafe { engine::weapon_type(weapon) })
            .map(|(object, weapon)| Restored {
                object,
                weapon,
                socket: record.socket,
            });
        let mut native = Native {
            owner,
            strong: Some(strong),
            carried: None,
        };
        let outcome = requested.map_or(Outcome::Refused(RestoreRefusal::NoSocket), |restored| {
            transfer(&mut native, restored)
        });
        let restored = matches!(outcome, Outcome::Restored(_));
        if restored {
            // SAFETY: the carry set keeps the successfully admitted weapon alive.
            unsafe { restore_rounds(native.carried.unwrap(), record) };
        } else {
            tracing::info!(target: "controllers", ?outcome, "inventory: a respawned weapon was not carried");
        }
        let mut status = STATUS.lock();
        if restored {
            status.restored += 1;
        }
        if let Some(outcome) = &mut status.last_respawn {
            if restored {
                outcome.restored += 1;
            } else {
                outcome.failed += 1;
            }
        }
    }
    let expired = expire_spawning();
    if !expired.is_empty() {
        tracing::info!(target: "controllers", expired = expired.len(), "inventory: respawns of carried weapons never came");
        if let Some(outcome) = &mut STATUS.lock().last_respawn {
            outcome.failed += expired.len();
        }
    }
    for record in expired.iter().filter(|r| r.single) {
        unsafe { single::pistol_not_restored(owner, record.tag_hash) };
    }
}

unsafe fn restore_rounds(object: usize, record: Record) {
    let weapon = unsafe { &mut *(object as *mut WeaponBase) };
    if record.single {
        unsafe { single::designate(weapon, "a carried pistol restored") };
    }
    if record.magazine >= 0 {
        unsafe { weapon.SetMagazineAmmo(record.magazine) };
    }
}
