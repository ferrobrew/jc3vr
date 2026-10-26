//! Weapon-owned chamber rounds partition the native loaded total without creating ammunition.
//!
//! The native count remains authoritative for shots, HUD updates, and handing control back to the
//! game. Only magazine-owned rounds travel with a retained magazine. An unretained magazine's
//! rounds remain in the native count for the game's next reload to return to the reserve, but
//! cannot feed the chamber or fire while the magazine is absent.

use parking_lot::Mutex;

use crate::controllers::weapons::{
    hold::refs::Strong,
    reload::{chamber::Chamber, records::MAX_RECORDS},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct NativeAmmo {
    pub total: i32,
    pub out: bool,
}

/// A partition of a weapon's native loaded rounds, never an additional source of rounds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Ammo {
    pub magazine: i32,
    pub chambers: [Chamber; 2],
    pub detached: i32,
    pub out: bool,
}

impl Ammo {
    pub(super) fn adopt(native: NativeAmmo, chambers: [Chamber; 2]) -> Self {
        let mut ammo = Self {
            magazine: 0,
            chambers,
            detached: 0,
            out: native.out,
        };
        ammo.reconcile(native);
        ammo
    }

    pub(super) fn total(self) -> i32 {
        self.magazine + self.chamber_rounds() + self.detached
    }

    pub(super) fn chamber_rounds(self) -> i32 {
        self.chambers
            .iter()
            .filter(|&&chamber| chamber == Chamber::Chambered)
            .count() as i32
    }

    /// Adopt native changes without adding rounds, or feeding an empty chamber implicitly.
    pub(super) fn reconcile(&mut self, native: NativeAmmo) {
        let mut remaining = native.total.max(0);
        for chamber in &mut self.chambers {
            if *chamber == Chamber::Chambered && remaining > 0 {
                remaining -= 1;
            } else {
                *chamber = Chamber::Empty;
            }
        }
        self.out = native.out;
        if native.out {
            self.magazine = 0;
            self.detached = remaining;
        } else {
            self.magazine = remaining;
            self.detached = 0;
        }
    }

    /// Commit only after the engine actually throws the magazine.
    pub(super) fn detach(&mut self, retained: bool) {
        if self.out {
            return;
        }
        if !retained {
            self.detached = self.magazine;
        }
        self.magazine = 0;
        self.out = true;
    }

    /// Replace a magazine, returning only the old magazine-owned rounds to the reserve.
    pub(super) fn insert(&mut self, rounds: i32) -> i32 {
        let returned = self.magazine + self.detached;
        self.magazine = rounds.max(0);
        self.detached = 0;
        self.out = false;
        returned
    }

    pub(super) fn reload_start(self) -> i32 {
        self.total() - self.chamber_rounds()
    }

    pub(super) fn reload_finish(&mut self, magazine: i32) -> i32 {
        self.insert(magazine);
        self.total()
    }

    pub(super) fn eject(&mut self, gun: usize) -> bool {
        let chamber = &mut self.chambers[gun.min(1)];
        let live = *chamber == Chamber::Chambered;
        *chamber = Chamber::Empty;
        live
    }

    pub(super) fn feed(&mut self, gun: usize) -> Chamber {
        let chamber = &mut self.chambers[gun.min(1)];
        if *chamber == Chamber::Empty && !self.out && self.magazine > 0 {
            self.magazine -= 1;
            *chamber = Chamber::Chambered;
        }
        *chamber
    }

    /// Native firing already consumes ammunition; partition its result, never consume it again.
    pub(super) fn shot(&mut self, gun: usize, total: i32, manual: bool) -> i32 {
        let before = self.total();
        let live = self.eject(gun);
        let total = if self.out && live {
            total.min(before - 1)
        } else {
            total
        };
        self.reconcile(NativeAmmo {
            total,
            out: self.out,
        });
        if !manual {
            self.feed(gun);
        }
        self.total()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Runtime {
    pub ammo: Ammo,
    guns: [Option<(usize, bool)>; 2],
}

impl Runtime {
    fn new(ammo: Ammo, guns: [Option<(usize, bool)>; 2]) -> Self {
        Self { ammo, guns }
    }

    fn remap(&mut self, guns: [Option<(usize, bool)>; 2]) {
        self.guns = guns;
    }

    fn shot(&mut self, hand: usize, total: i32) -> Option<i32> {
        let (gun, manual) = self.guns[hand]?;
        Some(self.ammo.shot(gun, total, manual))
    }

    fn holds_fire(self, hand: usize) -> bool {
        self.guns[hand].is_some_and(|(gun, _)| self.ammo.chambers[gun.min(1)] == Chamber::Empty)
    }

    fn suspend(&mut self) {
        self.guns = [None; 2];
    }
}

/// A live reference prevents an unrelated object at a reused address inheriting a chamber.
pub(super) fn observe(
    weapon: usize,
    owner: Strong,
    user: usize,
    native: NativeAmmo,
    chambers: [Chamber; 2],
    guns: [Option<(usize, bool)>; 2],
) -> Ammo {
    let mut ledger = LEDGER.lock();
    let mut retired = Vec::new();
    let mut incoming = Some(owner);
    let control = incoming.as_ref().unwrap().control_block() as usize;
    let entry = ledger
        .entries
        .iter()
        .position(|entry| entry.weapon == weapon);
    if let Some(index) = entry
        && (ledger.entries[index].owner.control_block() as usize != control
            || ledger.entries[index].user != user)
    {
        retired.push(ledger.entries.remove(index));
    }
    let index = match ledger
        .entries
        .iter()
        .position(|entry| entry.weapon == weapon)
    {
        Some(index) => index,
        None => {
            if ledger.entries.len() >= MAX_RECORDS {
                retired.push(ledger.entries.remove(0));
            }
            ledger.entries.push(Entry {
                weapon,
                owner: incoming.take().unwrap(),
                user,
                runtime: Runtime::new(Ammo::adopt(native, chambers), guns),
                magazine_only: false,
            });
            ledger.entries.len() - 1
        }
    };
    let mut entry = ledger.entries.remove(index);
    entry.runtime.ammo.reconcile(native);
    entry.runtime.remap(guns);
    let ammo = entry.runtime.ammo;
    ledger.entries.push(entry);
    drop(ledger);
    drop(incoming);
    drop(retired);
    ammo
}

/// A short lock only: no engine call or reload-state lock may run inside `change`.
pub(super) fn change<R>(weapon: usize, change: impl FnOnce(&mut Ammo) -> R) -> Option<R> {
    let mut ledger = LEDGER.lock();
    Some(change(
        &mut ledger
            .entries
            .iter_mut()
            .find(|e| e.weapon == weapon)?
            .runtime
            .ammo,
    ))
}

pub(super) fn snapshot(weapon: usize) -> Option<Ammo> {
    change(weapon, |ammo| *ammo)
}

pub(super) fn on_shot(weapon: usize, hand: usize, total: i32) -> Option<i32> {
    let mut ledger = LEDGER.lock();
    let entry = ledger.entries.iter_mut().find(|e| e.weapon == weapon)?;
    entry.runtime.shot(hand, total)
}

pub(super) fn holds_fire(weapon: usize, hand: usize) -> bool {
    let ledger = LEDGER.lock();
    ledger
        .entries
        .iter()
        .find(|e| e.weapon == weapon)
        .is_some_and(|entry| entry.runtime.holds_fire(hand))
}

pub(super) fn magazine_only(weapon: usize, active: bool) {
    if let Some(entry) = LEDGER
        .lock()
        .entries
        .iter_mut()
        .find(|entry| entry.weapon == weapon)
    {
        entry.magazine_only = active;
    }
}

pub(super) fn protected_rounds(weapon: usize) -> i32 {
    LEDGER
        .lock()
        .entries
        .iter()
        .find(|entry| entry.weapon == weapon)
        .filter(|entry| !entry.magazine_only)
        .map_or(0, |entry| entry.runtime.ammo.chamber_rounds())
}

/// Standdown relinquishes gating, not ammo. Every owned round is already native.
pub(super) fn suspend() {
    for entry in &mut LEDGER.lock().entries {
        entry.runtime.suspend();
    }
}

struct Entry {
    weapon: usize,
    owner: Strong,
    user: usize,
    runtime: Runtime,
    magazine_only: bool,
}

struct Ledger {
    entries: Vec<Entry>,
}

static LEDGER: Mutex<Ledger> = Mutex::new(Ledger {
    entries: Vec::new(),
});

#[cfg(test)]
mod tests;
