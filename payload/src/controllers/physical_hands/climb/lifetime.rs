//! Bounded lifetime stamps for gripped bodies, advanced before native destruction.
//!
//! Hash collisions conservatively lose a grip; they cannot make a destroyed body valid again.
//! No allocation or lock is needed on a Havok destruction or character worker.

use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn stamp(world: usize, body: u32) -> u64 {
    STAMPS[index(world, body)].load(Ordering::Acquire)
}

pub(crate) fn destroyed(world: usize, body: u32) {
    STAMPS[index(world, body)].fetch_add(1, Ordering::AcqRel);
}

fn index(world: usize, body: u32) -> usize {
    ((world >> 4) ^ body as usize) % BUCKETS
}

const BUCKETS: usize = 4096;
static STAMPS: [AtomicU64; BUCKETS] = [const { AtomicU64::new(0) }; BUCKETS];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_identical_recycled_metadata_has_a_different_lifetime() {
        let before = stamp(0x12340, 77);
        destroyed(0x12340, 77);
        assert_ne!(stamp(0x12340, 77), before);
    }
}
