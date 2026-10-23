//! What the reload remembers about each weapon across ticks and weapon changes: each gun's chamber
//! and action, the dual-wield split, and the magazine as last seen. Pure.
//!
//! A weapon is remembered by its loadout item ([`ItemId`]) while the loadout table holds it, so a
//! record survives the spawn reset's replacement of the weapon object with a new one of the same
//! type; a weapon the table does not hold is remembered by its object's address. A gun's entries
//! are indexed by the gun's index within its weapon, not by the hand holding it, so a weapon handed
//! to the other hand keeps its chambers. The records are bounded: past [`MAX_RECORDS`], the one
//! least recently in the hands is forgotten.

use crate::controllers::{
    loadout::ItemId,
    weapons::reload::{chamber::Chamber, dual::SplitMagazines, travel::ActionTravel},
};

/// Which weapon a record is for.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(super) enum RecordKey<I = ItemId> {
    /// The loadout table's item.
    Item(I),
    /// The weapon object's address, for a weapon the table does not hold.
    Object(usize),
}

impl<I> RecordKey<I> {
    /// The key of the weapon at `address`, which the table holds as `item`, when it does.
    pub(super) fn new(item: Option<I>, address: usize) -> Self {
        match item {
            Some(item) => RecordKey::Item(item),
            None => RecordKey::Object(address),
        }
    }
}

/// What the reload remembers about a weapon.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct Record {
    /// Each gun's chamber, by [`gun_entry`].
    pub chambers: [Chamber; GUN_ENTRIES],
    /// Each gun's action on its travel, by [`gun_entry`].
    pub travels: [ActionTravel; GUN_ENTRIES],
    pub split: Option<SplitMagazines>,
    /// The game's magazine as of the last tick.
    pub last_magazine: Option<i32>,
}

/// The entry of a record's per-gun arrays for the gun at `index` within its weapon. A weapon has
/// one gun, or two for the dual-wield pair; any further gun shares the last entry.
pub(super) fn gun_entry(index: usize) -> usize {
    index.min(GUN_ENTRIES - 1)
}

/// The remembered weapons, the most recently in the hands last.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Records<K = RecordKey> {
    entries: Vec<(K, Record)>,
}

impl<K: Copy + PartialEq> Records<K> {
    pub(super) const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// The record for `key`, made when there is none, forgetting the least recent past
    /// [`MAX_RECORDS`].
    pub(super) fn get(&mut self, key: K) -> &mut Record {
        let index = match self.entries.iter().position(|(k, _)| *k == key) {
            Some(index) => index,
            None => {
                if self.entries.len() >= MAX_RECORDS {
                    self.entries.remove(0);
                }
                self.entries.push((key, Record::default()));
                self.entries.len() - 1
            }
        };
        &mut self.entries[index].1
    }

    /// Mark `key`'s weapon as in the hands now, so it is the last to be forgotten. Makes its record
    /// when there is none.
    pub(super) fn touch(&mut self, key: K) {
        let record = *self.get(key);
        if let Some(index) = self.entries.iter().position(|(k, _)| *k == key) {
            self.entries.remove(index);
        }
        self.entries.push((key, record));
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Record> {
        self.entries.iter_mut().map(|(_, record)| record)
    }
}

/// How many weapons' records are kept across weapon changes.
pub(super) const MAX_RECORDS: usize = 8;
/// How many guns a record keeps entries for.
const GUN_ENTRIES: usize = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weapon_the_table_holds_is_keyed_by_its_item_across_objects() {
        assert_eq!(RecordKey::new(Some(7u32), 0x1000), RecordKey::Item(7));
        assert_eq!(
            RecordKey::new(Some(7u32), 0x1000),
            RecordKey::new(Some(7u32), 0x2000)
        );
        assert_eq!(
            RecordKey::<u32>::new(None, 0x1000),
            RecordKey::Object(0x1000)
        );
        assert_ne!(
            RecordKey::<u32>::new(None, 0x1000),
            RecordKey::<u32>::new(None, 0x2000)
        );
    }

    #[test]
    fn a_gun_past_the_second_shares_the_last_entry() {
        assert_eq!(gun_entry(0), 0);
        assert_eq!(gun_entry(1), 1);
        assert_eq!(gun_entry(5), 1);
    }

    #[test]
    fn a_record_is_kept_between_reads() {
        let mut records = Records::<usize>::new();
        records.get(1).chambers[1] = Chamber::Empty;
        assert_eq!(records.get(1).chambers[1], Chamber::Empty);
        assert_eq!(records.get(2).chambers[1], Chamber::default());
    }

    #[test]
    fn the_weapon_least_recently_in_the_hands_is_forgotten_first() {
        let mut records = Records::<usize>::new();
        for key in 0..MAX_RECORDS {
            records.get(key).last_magazine = Some(key as i32);
        }
        // The first weapon is back in the hands, so the second is now the oldest.
        records.touch(0);
        records.get(MAX_RECORDS);
        assert_eq!(records.get(0).last_magazine, Some(0));
        assert_eq!(records.get(1).last_magazine, None);
    }
}
