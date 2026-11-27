//! The adoption of a weapon the spawn system handed over for the restore or a give: it comes into
//! the carry set and onto its place in the table through one admission of the table's restore
//! ([`transfer`]), so that the socket it is carried on and the socket the table has it on are the
//! same, and a commit the table refuses lets it go again.

use jc3gi::{character::character::Character, equipment::weapon::WeaponBase};
use jc3vr_body::controllers::physics::refs::Strong;

use crate::controllers::{
    loadout::{
        Restored, Socket, live,
        restore::{RestoreAdmission, RestoreRefusal},
    },
    weapons::{
        dual_wield::single,
        inventory::{
            carry,
            carry::CarryRefusal,
            engine,
            engine::{kind_of, weapon_of},
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

/// Carry each respawned weapon the spawn callback handed over in the current generation onto its
/// record's socket, restoring its magazine, and count the spawns that never came. Runs only on an
/// input tick the table is ready, so it lands in the table the reset left.
pub(super) unsafe fn adopt(owner: &mut Character) {
    let generation = PLAN.lock().generation;
    let spawned = SPAWNED.take();
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
        let (outcome, carried) = match unsafe { engine::restored(weapon, record.socket) } {
            Some(restored) => carry_onto(owner, strong, restored),
            None => (Outcome::Refused(RestoreRefusal::NoSocket), None),
        };
        let restored = carried.is_some();
        if let Some(object) = carried {
            // SAFETY: the carry set keeps the successfully admitted weapon alive.
            unsafe { restore_rounds(object, record) };
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
