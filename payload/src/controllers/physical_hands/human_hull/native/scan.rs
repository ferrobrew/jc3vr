//! One pass over the live bodies: which hold a narrowed replacement, and which shapes to swap.

use std::{collections::HashMap, ptr::NonNull};

use jc3gi::havok::hknpShape;

use crate::controllers::physical_hands::human_hull::native::{Change, Table};

/// The adopted pairs keyed by each of their shapes, rebuilt whenever the tables change.
#[derive(Default)]
pub(super) struct Lookup {
    /// Each replacement's original.
    originals: HashMap<*const hknpShape, NonNull<hknpShape>>,
    /// Each original's replacement.
    replacements: HashMap<*const hknpShape, NonNull<hknpShape>>,
}

impl Lookup {
    pub(super) fn build(tables: &[Table]) -> Self {
        let mut lookup = Self::default();
        for pair in tables.iter().flat_map(|table| &table.pairs) {
            lookup
                .originals
                .insert(pair.replacement.as_ptr(), pair.original);
            lookup
                .replacements
                .insert(pair.original.as_ptr(), pair.replacement);
        }
        lookup
    }

    /// The original of `shape`, when `shape` is a replacement.
    pub(super) fn original_of(&self, shape: *const hknpShape) -> Option<NonNull<hknpShape>> {
        self.originals.get(&shape).copied()
    }

    fn replacement_of(&self, shape: *const hknpShape) -> Option<NonNull<hknpShape>> {
        self.replacements.get(&shape).copied()
    }
}

/// A live body that holds a narrowed replacement.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Migrated {
    pub(super) id: u32,
    pub(super) original: NonNull<hknpShape>,
    pub(super) replacement: NonNull<hknpShape>,
}

/// What one pass over the live bodies found.
#[derive(Debug, Default)]
pub(super) struct Scan {
    /// Every live body that holds a replacement.
    pub(super) migrated: Vec<Migrated>,
    /// The bodies that hold an original and would take its replacement, for a narrowing pass.
    pub(super) narrowing: Vec<Change>,
}

/// Classify each live body `(id, shape)` against the adopted pairs in one lookup per body.
pub(super) fn scan(
    bodies: impl IntoIterator<Item = (u32, *const hknpShape)>,
    lookup: &Lookup,
) -> Scan {
    let mut scan = Scan::default();
    for (id, shape) in bodies {
        if let Some(original) = lookup.original_of(shape) {
            scan.migrated.push(Migrated {
                id,
                original,
                // SAFETY: the lookup's keys are the pairs' non-null shapes.
                replacement: unsafe { NonNull::new_unchecked(shape.cast_mut()) },
            });
        } else if let Some(replacement) = lookup.replacement_of(shape) {
            scan.narrowing.push(Change {
                id,
                // SAFETY: as above.
                from: unsafe { NonNull::new_unchecked(shape.cast_mut()) },
                to: replacement,
            });
        }
    }
    scan
}

/// Paces the discovery walk, which looks up every live body's owner. Construction hooks adopt new
/// settings tables as they are made, so the walk only has to find tables that existed before the
/// hooks did and whose characters stream in later: it runs on every pass until a table is adopted,
/// then once every [`DISCOVERY_INTERVAL_PASSES`].
#[derive(Debug, Default)]
pub(super) struct DiscoveryClock {
    passes_since: u32,
}

impl DiscoveryClock {
    pub(super) fn due(&mut self, have_tables: bool) -> bool {
        if !have_tables || self.passes_since == 0 {
            self.passes_since = DISCOVERY_INTERVAL_PASSES - 1;
            true
        } else {
            self.passes_since -= 1;
            false
        }
    }

    pub(super) const fn new() -> Self {
        Self { passes_since: 0 }
    }
}

/// The longest a table whose characters stream in waits to be found, in pre-simulation passes.
const DISCOVERY_INTERVAL_PASSES: u32 = 60;

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(address: usize) -> NonNull<hknpShape> {
        NonNull::new(address as *mut hknpShape).unwrap()
    }

    fn lookup(pairs: &[(usize, usize)]) -> Lookup {
        let mut lookup = Lookup::default();
        for &(original, replacement) in pairs {
            lookup
                .originals
                .insert(shape(replacement).as_ptr(), shape(original));
            lookup
                .replacements
                .insert(shape(original).as_ptr(), shape(replacement));
        }
        lookup
    }

    #[test]
    fn one_pass_finds_the_migrated_bodies_and_the_ones_to_narrow() {
        let lookup = lookup(&[(0x100, 0x200), (0x110, 0x210)]);
        let scan = scan(
            [
                (1, shape(0x200).as_ptr().cast_const()),
                (2, shape(0x110).as_ptr().cast_const()),
                (3, shape(0x999).as_ptr().cast_const()),
                (4, shape(0x210).as_ptr().cast_const()),
            ],
            &lookup,
        );
        assert_eq!(
            scan.migrated,
            [
                Migrated {
                    id: 1,
                    original: shape(0x100),
                    replacement: shape(0x200)
                },
                Migrated {
                    id: 4,
                    original: shape(0x110),
                    replacement: shape(0x210)
                },
            ]
        );
        assert_eq!(scan.narrowing.len(), 1);
        assert_eq!(scan.narrowing[0].id, 2);
        assert_eq!(scan.narrowing[0].to, shape(0x210));
    }

    #[test]
    fn discovery_runs_every_pass_until_a_table_exists_then_at_its_interval() {
        let mut clock = DiscoveryClock::new();
        assert!(clock.due(false));
        assert!(clock.due(false));
        let runs = (0..DISCOVERY_INTERVAL_PASSES * 3)
            .filter(|_| clock.due(true))
            .count();
        assert_eq!(runs, 3);
        // A forgotten last table brings discovery back at once.
        assert!(clock.due(false));
    }
}
