//! What the hooks and the hands tell the table, and the fold that turns each into transitions.
//! Pure.
//!
//! A hook decides nothing: it turns what the game wanted (a pickup, a give, a removal, a store
//! about to happen, a cache, a reset) into an [`Event`], and the hands' grabs and releases on the
//! input tick do the same. [`Loadout::fold`] applies one event whole or refuses it, and returns
//! what changed for the log and, for a store or a cache, what the game's slots and the sidecar
//! should hold before the game goes on.

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::ObjectAddress,
        item::ItemId,
        projection::{StoreProjection, project_store},
        reset::{CacheKind, ResetKind, SlotObject},
        socket::Socket,
        table::Loadout,
        transition::{
            Give, LostWhy, Release, Removal, RemovalWhy, Restored, Take, Transition,
            TransitionError,
        },
    },
};

/// One thing that happened to the player's items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A hand took an object: off the ground, a rack, a locker, a character, a give, the grapple
    /// snatch, a pouch, or out of the air.
    Take(Take),
    /// A hand opened on a socket that fits what it holds.
    Dock { hand: Hand, socket: Socket },
    /// An empty hand closed on a docked item.
    Draw { socket: Socket, hand: Hand },
    /// The empty hand closed on what the other hand holds.
    HandOver { from: Hand },
    /// A hand opened away from any socket.
    LetGo { hand: Hand, release: Release },
    /// A weapon lying in the world was lost rather than dropped.
    Lost { object: ObjectAddress, why: LostWhy },
    /// The game gave a weapon.
    Given(Give),
    /// A restored item's object was spawned.
    Spawned { item: ItemId, object: ObjectAddress },
    /// The game took items away.
    Removed { target: Removal, why: RemovalWhy },
    /// The game let go of an object the player had.
    Dropped { object: ObjectAddress },
    /// The mod spawned a weapon to stand for one the player has.
    Restored(Restored),
    /// The game is about to store its save.
    StoreImminent,
    /// The game is about to cache the player's slots.
    Cached(CacheKind),
    /// The game restored a cache, and its slots hold `slots`.
    CacheRestored {
        cache: CacheKind,
        slots: Vec<SlotObject>,
    },
    /// The spawn reset is about to replace the slots' weapons.
    ResetBegan,
    /// The spawn reset is done, and the game's slots hold `slots`.
    ResetCompleted {
        kind: ResetKind,
        slots: Vec<SlotObject>,
    },
}

/// What folding one event did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Folded {
    pub transitions: Vec<Transition>,
    /// For a store or a cache: what the game's slots and the sidecar should hold before the game
    /// goes on.
    pub store: Option<StoreProjection>,
}

impl Folded {
    fn moves(transitions: Vec<Transition>) -> Folded {
        Folded {
            transitions,
            store: None,
        }
    }
}

impl Loadout {
    /// Apply `event`, whole or not at all.
    pub fn fold(&mut self, event: Event) -> Result<Folded, TransitionError> {
        Ok(match event {
            Event::Take(take) => Folded::moves(self.take(take)?),
            Event::Dock { hand, socket } => Folded::moves(self.dock(hand, socket)?),
            Event::Draw { socket, hand } => Folded::moves(self.draw(socket, hand)?),
            Event::HandOver { from } => Folded::moves(self.hand_over(from)?),
            Event::LetGo { hand, release } => Folded::moves(self.let_go(hand, release)?),
            Event::Lost { object, why } => Folded::moves(self.lose(object, why)?),
            Event::Given(give) => Folded::moves(self.give(give)?),
            Event::Spawned { item, object } => Folded::moves(self.spawned(item, object)?),
            Event::Removed { target, why } => Folded::moves(self.remove(target, why)?),
            Event::Dropped { object } => Folded::moves(self.dropped(object)?),
            Event::Restored(restored) => Folded::moves(self.restored(restored)?),
            Event::StoreImminent => Folded {
                transitions: Vec::new(),
                store: Some(project_store(self)),
            },
            Event::Cached(kind) => Folded {
                transitions: Vec::new(),
                store: Some(self.cache(kind)),
            },
            Event::CacheRestored { cache, slots } => Folded::moves(self.restore(cache, &slots)?),
            Event::ResetBegan => Folded::moves(self.begin_reset()),
            Event::ResetCompleted { kind, slots } => {
                Folded::moves(self.complete_reset(kind, &slots)?)
            }
        })
    }
}
