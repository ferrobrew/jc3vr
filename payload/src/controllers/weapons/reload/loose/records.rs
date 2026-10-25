//! Fixed-capacity admission for magazine rounds transferred by the mod.
//!
//! Reservations and committed records occupy the same slots. Copied world handles include the
//! slot generation, so reusing native fields cannot give an old result ownership of a new item.

use std::time::Instant;

use crate::controllers::weapons::reload::loose::{Captured, Loose, Maintenance, Operation};

pub(super) const CAPACITY: usize = 32;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct DropKey {
    pub gun: usize,
    pub spare: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Token {
    slot: usize,
    generation: u64,
}

/// Committed ownership identity. Only the store can create a production handle.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct WorldHandle(Token);

impl WorldHandle {
    #[cfg(test)]
    pub(crate) const fn test(generation: u64) -> Self {
        Self(Token {
            slot: 0,
            generation,
        })
    }
}

pub(super) struct Records {
    slots: [Option<Entry>; CAPACITY],
    generation: u64,
}

impl Records {
    pub(super) const fn new() -> Self {
        Self {
            slots: [None; CAPACITY],
            generation: 0,
        }
    }

    /// Reject duplicates and saturation before any engine operation or ammunition transfer.
    pub(super) fn reserve(&mut self, key: DropKey) -> Option<Token> {
        if key.gun == 0
            || key.spare == 0
            || self.slots.iter().flatten().any(|entry| match entry {
                Entry::Reserved { key: old, .. } => old.spare == key.spare,
                Entry::World { magazine, .. } => magazine.object.get() == key.spare,
            })
        {
            return None;
        }
        let slot = self.slots.iter().position(Option::is_none)?;
        let generation = self.generation.checked_add(1)?;
        self.generation = generation;
        let token = Token { slot, generation };
        self.slots[slot] = Some(Entry::Reserved {
            token,
            key,
            executing: false,
        });
        Some(token)
    }

    pub(super) fn matches(&self, token: Token, key: DropKey) -> bool {
        matches!(self.slots.get(token.slot), Some(Some(Entry::Reserved {
            token: actual, key: expected, executing: false,
        })) if *actual == token && *expected == key)
    }

    /// A reentrant or duplicate throw cannot consume the same reservation twice.
    pub(super) fn begin(&mut self, token: Token, key: DropKey) -> bool {
        if !self.matches(token, key) {
            return false;
        }
        self.slots[token.slot] = Some(Entry::Reserved {
            token,
            key,
            executing: true,
        });
        true
    }

    pub(super) fn commit(
        &mut self,
        token: Token,
        captured: Captured,
        now: Instant,
    ) -> Option<Loose> {
        let Some(Entry::Reserved {
            token: actual,
            key,
            executing: true,
        }) = self.slots[token.slot]
        else {
            return None;
        };
        if actual != token || key.spare != captured.object.get() {
            return None;
        }
        let Captured {
            object,
            body,
            owner,
            tag,
            rounds,
        } = captured;
        let magazine = Loose {
            handle: WorldHandle(token),
            object,
            body,
            owner,
            tag,
            rounds,
        };
        self.slots[token.slot] = Some(Entry::World {
            token,
            magazine,
            dropped: now,
            leased: true,
            usable: true,
        });
        Some(magazine)
    }

    /// Dropping a result lease never removes its committed world item or reverses its rounds.
    pub(super) fn release(&mut self, token: Token) {
        match self.slots[token.slot] {
            Some(Entry::Reserved { token: actual, .. }) if actual == token => {
                self.slots[token.slot] = None
            }
            Some(Entry::World {
                token: actual,
                magazine,
                dropped,
                usable,
                ..
            }) if actual == token => {
                self.slots[token.slot] = Some(Entry::World {
                    token,
                    magazine,
                    dropped,
                    usable,
                    leased: false,
                });
            }
            _ => {}
        }
    }

    pub(super) fn magazines(&self) -> impl Iterator<Item = Loose> + '_ {
        self.slots.iter().filter_map(|entry| match entry {
            Some(Entry::World { magazine, .. }) => Some(*magazine),
            _ => None,
        })
    }

    pub(super) fn contains(&self, magazine: Loose) -> bool {
        matches!(self.slots.get(magazine.handle.0.slot), Some(Some(Entry::World {
            token, magazine: actual, ..
        })) if *token == magazine.handle.0 && *actual == magazine)
    }

    pub(super) fn usable(&self, magazine: Loose) -> bool {
        self.contains(magazine)
            && matches!(
                self.slots[magazine.handle.0.slot],
                Some(Entry::World { usable: true, .. })
            )
    }

    /// Apply every body observation only to its matching committed generation.
    pub(super) fn observe(&mut self, magazine: Loose, status: Maintenance) -> Operation<()> {
        if !self.contains(magazine) {
            return Operation::Gone;
        }
        let slot = &mut self.slots[magazine.handle.0.slot];
        match status {
            #[cfg(test)]
            Maintenance::Gone => {
                *slot = None;
                Operation::Gone
            }
            Maintenance::Unavailable { identity_lost } => {
                if identity_lost && let Some(Entry::World { usable, .. }) = slot {
                    *usable = false;
                }
                Operation::Deferred
            }
            Maintenance::Live => match slot {
                Some(Entry::World { usable: true, .. }) => Operation::Ready(()),
                _ => Operation::Deferred,
            },
        }
    }

    /// Seating consumes only the committed generation that supplied the held snapshot.
    pub(super) fn remove(&mut self, magazine: Loose) -> bool {
        if !self.usable(magazine) {
            return false;
        }
        self.slots[magazine.handle.0.slot] = None;
        true
    }

    pub(super) fn maintain(
        &mut self,
        now: Instant,
        keep_s: f32,
        held: Option<WorldHandle>,
        mut status: impl FnMut(Loose) -> Maintenance,
    ) {
        for index in 0..CAPACITY {
            let Some(Entry::World {
                magazine,
                dropped,
                leased,
                ..
            }) = self.slots[index]
            else {
                continue;
            };
            if self.observe(magazine, status(magazine)) != Operation::Ready(()) {
                continue;
            }
            let expired = magazine.rounds == 0
                && !leased
                && Some(magazine.handle) != held
                && now.saturating_duration_since(dropped).as_secs_f32() > keep_s;
            if expired {
                self.slots[index] = None;
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum Entry {
    Reserved {
        token: Token,
        key: DropKey,
        executing: bool,
    },
    World {
        token: Token,
        magazine: Loose,
        dropped: Instant,
        leased: bool,
        usable: bool,
    },
}

#[cfg(test)]
mod tests;
