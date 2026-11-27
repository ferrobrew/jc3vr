//! Lifetime stamps for the bodies a hand grips, advanced before the world destroys a body
//! ([`destroyed`]) and compared against the stamp a grip took ([`stamp`]).
//!
//! The stamps are a fixed table of atomics hashed by world and body, so a Havok destruction worker
//! or a character worker reads and advances them without an allocation or a lock. A collision
//! only loses a grip; it cannot make a destroyed body valid again.

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
