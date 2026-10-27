//! The live table as the placement and the other detours read it: each hand's weapon, lock-free,
//! and the place of every item the table has on the body.
//!
//! The detours run on the simulation's worker threads for every character, so the hands' weapons
//! are published as one object address per hand ([`hand_of`], [`in_hand`]), compared before any
//! lock. What each hand holds of any kind ([`held_kind`]) is behind a leaf lock, for the hands that
//! must not take anything else.
//! The places on the body and the items there ([`place_of`], [`item_of`]) are behind a leaf lock
//! of their own, which only the local
//! player's placement takes. Both are written from the table under its lock, after every fold that
//! can move an item: at the end of each input tick, and at once for the moves the hands make
//! between ticks, so the next placement already follows them.

use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::ObjectAddress,
        item::{ItemId, ItemKind},
        table::{Loadout, Place},
    },
};

/// The hand holding the weapon at `address`, when the table has it in one.
pub(crate) fn hand_of(address: usize) -> Option<Hand> {
    if address == 0 {
        return None;
    }
    Hand::BOTH
        .into_iter()
        .find(|hand| HANDS[hand.index()].load(Ordering::Relaxed) == address)
}

/// What the table has in `hand`, of any kind, when it has an item there.
pub(crate) fn held_kind(hand: Hand) -> Option<ItemKind> {
    HELD.lock()[hand.index()]
}

/// The weapon in `hand`, when the table has one there.
pub(crate) fn in_hand(hand: Hand) -> Option<usize> {
    match HANDS[hand.index()].load(Ordering::Relaxed) {
        0 => None,
        address => Some(address),
    }
}

/// Where on the body the table has the object at `address`: a hand or a socket.
pub(crate) fn place_of(address: usize) -> Option<Place> {
    PLACES
        .lock()
        .iter()
        .find(|(object, _, _)| object.get() == address)
        .map(|&(_, _, place)| place)
}

/// The item the table binds to the object at `address` on the body.
pub(crate) fn item_of(address: usize) -> Option<ItemId> {
    PLACES
        .lock()
        .iter()
        .find(|(object, _, _)| object.get() == address)
        .map(|&(_, item, _)| item)
}

/// Publish `table`'s hands and its places on the body.
pub(super) fn publish(table: &Loadout) {
    let weapons = Hand::BOTH.map(|hand| {
        table
            .held(hand)
            .filter(|item| matches!(item.kind, ItemKind::Weapon(_)))
            .and_then(|item| item.binding.address())
            .map_or(0, ObjectAddress::get)
    });
    if Hand::BOTH
        .into_iter()
        .any(|hand| HANDS[hand.index()].load(Ordering::Relaxed) != weapons[hand.index()])
    {
        crate::controllers::weapons::hold::take::invalidate_handle_placements();
    }
    for hand in Hand::BOTH {
        HANDS[hand.index()].store(weapons[hand.index()], Ordering::Relaxed);
    }
    *HELD.lock() = Hand::BOTH.map(|hand| table.held(hand).map(|item| item.kind));
    let mut places = PLACES.lock();
    places.clear();
    places.extend(table.items().filter_map(|item| {
        let object = item.binding.address()?;
        match item.place {
            Place::Hand(_) | Place::Socket(_) => Some((object, item.id, item.place)),
            Place::World(_) => None,
        }
    }));
}

/// Publish an empty table: the table stopped.
pub(super) fn clear() {
    crate::controllers::weapons::hold::take::invalidate_handle_placements();
    for hand in &HANDS {
        hand.store(0, Ordering::Relaxed);
    }
    *HELD.lock() = [None; 2];
    PLACES.lock().clear();
}

static HANDS: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
/// What each hand holds, of any kind, by [`Hand::index`].
static HELD: Mutex<[Option<ItemKind>; 2]> = Mutex::new([None; 2]);
static PLACES: Mutex<Vec<(ObjectAddress, ItemId, Place)>> = Mutex::new(Vec::new());

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::controllers::loadout::{BodyLayout, Departure, safe_items::ItemLeaseId};

    #[test]
    fn safe_item_moves_publish_hand_occupancy_without_a_native_alias() {
        let mut table = Loadout::new(BodyLayout::full(Hand::Right));
        let item = table
            .admit_safe_item(ItemLeaseId::new(NonZeroU64::MIN), Hand::Left)
            .unwrap();
        publish(&table);
        assert_eq!(held_kind(Hand::Left), Some(ItemKind::Grenade));
        assert_eq!(in_hand(Hand::Left), None);
        assert!(PLACES.lock().is_empty());
        let item = table
            .move_safe_item(item, Place::Hand(Hand::Right))
            .unwrap();
        publish(&table);
        assert_eq!(held_kind(Hand::Left), None);
        assert_eq!(held_kind(Hand::Right), Some(ItemKind::Grenade));
        assert_eq!(in_hand(Hand::Right), None);
        let item = table
            .move_safe_item(item, Place::World(Departure::Dropped))
            .unwrap();
        publish(&table);
        assert_eq!(held_kind(Hand::Right), None);
        assert_eq!(table.retire_safe_item(item), Ok(item.binding.lease));
        publish(&table);
        assert!(PLACES.lock().is_empty());
        clear();
    }
}
