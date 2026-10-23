//! The spawn reset's re-keying, and the table's own snapshots at the game's weapon caches. Pure.
//!
//! Every spawn reset replaces each slot's weapon with a new object of the same type. Before it,
//! the items the game's slots hold (by the slot projection) are unbound and wait for their slot's
//! new object ([`Loadout::begin_reset`]); after it, each new slot object re-binds the item that
//! waits for it, so the item keeps its identity and its place ([`Loadout::complete_reset`]).
//!
//! The game caches the slots at a mission's every checkpoint, before a mission takes the player's
//! weapons, and for a challenge, and restores the cache later. The table takes a snapshot of its own
//! at each cache ([`Loadout::cache`]) and restores it when the game restores the cache
//! ([`Loadout::restore`]).

use std::fmt;

use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Awaiting, Binding, ObjectAddress},
        item::{ItemId, ItemKind, WeaponType},
        projection::{
            BodyPlace, ProjectionMode, SlotContent, StoreProjection, project, project_store,
        },
        table::{Docked, Item, Loadout, Place},
        transition::{Cause, Change, Transition, TransitionError, change, left},
    },
};

/// What the spawn reset was for, which decides what the hands keep.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ResetKind {
    /// A death, a mission's failure, or a checkpoint restart: the game's rules decide what the
    /// player owns, and the hands come back empty with everything else on its socket.
    Respawn,
    /// A fast travel, or a challenge's or a mission's start: the player keeps what the hands hold.
    Travel,
    /// A save loaded, or the frontend: the table starts again from the game's slots.
    Load,
}

impl ResetKind {
    pub const ALL: [ResetKind; 3] = [ResetKind::Respawn, ResetKind::Travel, ResetKind::Load];

    pub const fn label(self) -> &'static str {
        match self {
            ResetKind::Respawn => "respawn",
            ResetKind::Travel => "travel",
            ResetKind::Load => "load",
        }
    }
}

/// One of the game's weapon caches.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CacheKind {
    /// The mission manager's cache at each checkpoint, restored on a death, a failure, or a
    /// checkpoint restart: the table returns to its snapshot.
    Checkpoint,
    /// A mission's cache, taken as it takes the player's weapons and restored as it gives them
    /// back: the snapshot's weapons come back beside what the player holds since.
    Mission,
    /// A challenge's cache, restored as for a mission.
    Challenge,
}

impl CacheKind {
    pub const COUNT: usize = 3;

    pub const ALL: [CacheKind; CacheKind::COUNT] = [
        CacheKind::Checkpoint,
        CacheKind::Mission,
        CacheKind::Challenge,
    ];

    const fn index(self) -> usize {
        match self {
            CacheKind::Checkpoint => 0,
            CacheKind::Mission => 1,
            CacheKind::Challenge => 2,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            CacheKind::Checkpoint => "checkpoint snapshot",
            CacheKind::Mission => "mission's weapon cache",
            CacheKind::Challenge => "challenge's weapon cache",
        }
    }
}

impl fmt::Display for CacheKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A weapon the game's slots hold, as a reset or a restore reports them.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SlotObject {
    pub object: ObjectAddress,
    pub weapon: WeaponType,
}

/// The table's snapshots, one per cache.
#[derive(Debug)]
pub(super) struct Caches([Option<Vec<Cached>>; CacheKind::COUNT]);

impl Caches {
    pub(super) const fn new() -> Caches {
        Caches([const { None }; CacheKind::COUNT])
    }

    /// Which caches hold a snapshot.
    pub(super) fn held(&self) -> impl Iterator<Item = CacheKind> + '_ {
        CacheKind::ALL
            .into_iter()
            .filter(|kind| self.0[kind.index()].is_some())
    }
}

/// One item of a snapshot: a weapon on the body, and the slot the store projection put it in.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Cached {
    id: ItemId,
    weapon: WeaponType,
    place: BodyPlace,
    slot: Option<WeaponSlot>,
}

impl Loadout {
    /// The spawn reset is about to replace the slots' weapons: every item the game's slots hold
    /// now waits for its slot's new object. A second begin before the reset completes changes
    /// nothing.
    pub fn begin_reset(&mut self) -> Vec<Transition> {
        let projection = project(self, ProjectionMode::Play);
        let mut out = Vec::new();
        for fill in projection.slots {
            let SlotContent::Weapon { item, .. } = fill.content else {
                continue;
            };
            let awaiting = Awaiting::Slot(fill.slot);
            // A slot an item already waits for is pending from a begin that has not completed
            // yet; a second begin leaves it so.
            if self
                .items()
                .any(|i| i.binding == Binding::Awaiting(awaiting))
            {
                continue;
            }
            if let Some(entry) = self.item_mut(item)
                && let Binding::Live(_) = entry.binding
            {
                entry.binding = Binding::Awaiting(awaiting);
                out.push(Transition {
                    item,
                    kind: entry.kind,
                    change: Change::Unbound { awaiting },
                    cause: Cause::ResetBegan,
                });
            }
        }
        out
    }

    /// The spawn reset is done, and the game's slots hold `slots`. The weapons in the world are
    /// gone; each slot's new object re-binds the item that waits for that slot with the same type,
    /// and a new object nothing waits for goes onto the body; an item whose slot the game emptied
    /// or gave another type leaves the table. What the hands keep follows `kind`.
    pub fn complete_reset(
        &mut self,
        kind: ResetKind,
        slots: &[SlotObject],
    ) -> Result<Vec<Transition>, TransitionError> {
        check_slots(slots)?;
        let cause = Cause::Reset(kind);
        let mut out = match kind {
            ResetKind::Load => self.remove_where(|_| true, cause),
            ResetKind::Respawn | ResetKind::Travel => self.clear_world(cause),
        };
        if kind == ResetKind::Respawn {
            out.extend(self.empty_hands(cause));
        }
        for slot in slots {
            if self.locate(slot.object).is_some() {
                // The game kept this object.
                continue;
            }
            let waiting = self.awaiting_slot(*slot);
            match waiting.and_then(|id| self.item_mut(id)) {
                Some(entry) => {
                    entry.binding = Binding::Live(slot.object);
                    out.push(Transition {
                        item: entry.id,
                        kind: entry.kind,
                        change: Change::Bound {
                            object: slot.object,
                        },
                        cause,
                    });
                }
                None => {
                    let item = self.new_item(*slot);
                    out.extend(self.onto_body(item, None, cause));
                }
            }
        }
        out.extend(self.remove_where(
            |item| matches!(item.binding, Binding::Awaiting(Awaiting::Slot(_))),
            cause,
        ));
        Ok(out)
    }

    /// The game caches its slots: take the table's own snapshot of the weapons on the body, and
    /// return what the slots should hold for the cache and what the sidecar records.
    pub fn cache(&mut self, kind: CacheKind) -> StoreProjection {
        let store = project_store(self);
        let snapshot = store
            .sidecar
            .iter()
            .map(|entry| Cached {
                id: entry.item,
                weapon: entry.weapon,
                place: entry.place,
                slot: entry.slot,
            })
            .collect();
        self.caches.0[kind.index()] = Some(snapshot);
        store
    }

    /// The game restored its cache, and its slots hold `slots`. A checkpoint's restore returns the
    /// table to its snapshot: what the player gained since leaves the table. A mission's or a
    /// challenge's brings the snapshot's weapons back beside what the player holds since. Either
    /// way the hands come back empty: a weapon the snapshot had in a hand goes onto a socket, and a
    /// snapshot's weapon whose object is gone waits to be spawned again.
    pub fn restore(
        &mut self,
        kind: CacheKind,
        slots: &[SlotObject],
    ) -> Result<Vec<Transition>, TransitionError> {
        check_slots(slots)?;
        let snapshot = self.caches.0[kind.index()]
            .clone()
            .ok_or(TransitionError::NoCache(kind))?;
        let cause = Cause::Restore(kind);
        let mut out = Vec::new();
        if kind == CacheKind::Checkpoint {
            out.extend(self.remove_where(
                |item| !snapshot.iter().any(|cached| cached.id == item.id),
                cause,
            ));
        }
        let mut claimed: Vec<ObjectAddress> = slots
            .iter()
            .map(|slot| slot.object)
            .filter(|&object| self.locate(object).is_some())
            .collect();
        // A checkpoint's items all come out before any goes back, so that two that swapped places
        // since the snapshot swap back rather than block each other.
        let mut pool = Vec::new();
        if kind == CacheKind::Checkpoint {
            for cached in &snapshot {
                if let Some(found) = self.locate_item(cached.id) {
                    pool.push(self.remove_at(found)?);
                }
            }
        }
        // The sockets' weapons first, so a hand's weapon takes a socket only once they have theirs.
        let ordered = snapshot
            .iter()
            .filter(|c| matches!(c.place, BodyPlace::Socket(_)))
            .chain(
                snapshot
                    .iter()
                    .filter(|c| matches!(c.place, BodyPlace::Hand(_))),
            );
        for cached in ordered {
            let (mut item, from) = match pool.iter().position(|(item, _)| item.id == cached.id) {
                Some(index) => {
                    let (item, from) = pool.swap_remove(index);
                    (item, Some(from))
                }
                // A mission's give-back leaves what is still in the table where the player has it.
                None if self.locate_item(cached.id).is_some() => continue,
                None => (
                    Item {
                        id: cached.id,
                        kind: ItemKind::Weapon(cached.weapon),
                        binding: Binding::Awaiting(Awaiting::Respawn),
                    },
                    None,
                ),
            };
            let object = slots
                .iter()
                .find(|slot| {
                    !claimed.contains(&slot.object)
                        && Some(slot.weapon.slot()) == cached.slot
                        && slot.weapon.tag_hash() == cached.weapon.tag_hash()
                })
                .map(|slot| slot.object);
            if let (Binding::Awaiting(_), Some(object)) = (item.binding, object) {
                claimed.push(object);
                item.binding = Binding::Live(object);
                if from.is_some() {
                    out.push(Transition {
                        item: item.id,
                        kind: item.kind,
                        change: Change::Bound { object },
                        cause,
                    });
                }
            }
            out.extend(self.put_back(item, cached.place, from, cause));
        }
        for slot in slots {
            if claimed.contains(&slot.object) {
                continue;
            }
            let item = self.new_item(*slot);
            out.extend(match kind {
                CacheKind::Checkpoint => self.onto_body(item, None, cause),
                CacheKind::Mission | CacheKind::Challenge => self.give_item(item, None, cause),
            });
        }
        Ok(out)
    }

    /// Which snapshots the table holds.
    pub fn caches_held(&self) -> impl Iterator<Item = CacheKind> + '_ {
        self.caches.held()
    }

    /// Put a restored item back: on its socket when that is free, otherwise as a lost weapon
    /// returns. A hand's item goes onto a socket, since the hands come back empty.
    fn put_back(
        &mut self,
        item: Item,
        place: BodyPlace,
        from: Option<Place>,
        cause: Cause,
    ) -> Option<Transition> {
        let item = match place {
            BodyPlace::Socket(socket) if self.sockets[socket.index()].is_none() => {
                match Docked::new(item, socket) {
                    Ok(docked) => {
                        let transition = change(&docked.item, from, Place::Socket(socket), cause);
                        self.sockets[socket.index()] = Some(docked);
                        return (from != Some(Place::Socket(socket))).then_some(transition);
                    }
                    Err(item) => item,
                }
            }
            BodyPlace::Socket(_) | BodyPlace::Hand(_) => item,
        };
        self.onto_body(item, from, cause)
    }

    /// Empty both hands for a respawn: a weapon goes onto a socket, and anything else leaves.
    fn empty_hands(&mut self, cause: Cause) -> Vec<Transition> {
        let mut out = Vec::new();
        for hand in Hand::BOTH {
            let Some(item) = self.hands[hand.index()].take() else {
                continue;
            };
            let from = Some(Place::Hand(hand));
            out.extend(match item.kind {
                ItemKind::Weapon(_) => self.onto_body(item, from, cause),
                ItemKind::Prop | ItemKind::Grenade | ItemKind::Charge => {
                    Some(left(&item, Place::Hand(hand), cause))
                }
            });
        }
        out
    }

    /// Every weapon lying in the world leaves the table.
    fn clear_world(&mut self, cause: Cause) -> Vec<Transition> {
        std::mem::take(&mut self.world)
            .into_iter()
            .map(|lying| left(&lying.item, Place::World(lying.departure), cause))
            .collect()
    }

    /// The item waiting for `slot`'s new object: unbound for its slot, of its type.
    fn awaiting_slot(&self, slot: SlotObject) -> Option<ItemId> {
        self.items()
            .find(|item| {
                item.binding == Binding::Awaiting(Awaiting::Slot(slot.weapon.slot()))
                    && item.kind.weapon().map(WeaponType::tag_hash) == Some(slot.weapon.tag_hash())
            })
            .map(|item| item.id)
    }

    fn new_item(&mut self, slot: SlotObject) -> Item {
        Item {
            id: self.allocate(),
            kind: ItemKind::Weapon(slot.weapon),
            binding: Binding::Live(slot.object),
        }
    }
}

/// Refuse a report of the game's slots that names one object, or one slot, twice.
fn check_slots(slots: &[SlotObject]) -> Result<(), TransitionError> {
    for (index, slot) in slots.iter().enumerate() {
        let twice = slots[..index].iter().any(|earlier| {
            earlier.object == slot.object || earlier.weapon.slot() == slot.weapon.slot()
        });
        if twice {
            return Err(TransitionError::DuplicateSlotObject(slot.object));
        }
    }
    Ok(())
}
