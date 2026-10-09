//! The retained per-source slots.
//!
//! Producers run at different rates on different threads: the sim tick, the character workers, the
//! main thread's frame loop, and the render thread. The renderer draws at frame rate. A list
//! cleared every rendered frame would flicker for the slower producers, so each source instead owns
//! a slot that its submission replaces wholesale and stamps. The renderer draws a slot until its
//! time to live runs out, so a producer that stops submitting (its toggle turned off, a mode change,
//! a menu) disappears on its own.
//!
//! The global store's lock is a leaf: nothing else is locked or called while it is held, so any
//! thread may submit.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use crate::controllers::overlay::primitive::{Primitive, Source};

/// The most primitives the renderer draws in a frame; the rest are dropped.
pub const MAX_PRIMITIVES: usize = 4096;

/// Replace `source`'s slot with `primitives`, drawn until `ttl` elapses. The slot's previous
/// primitives are swapped into `primitives`, so a producer can reuse the allocation.
pub fn submit(source: Source, primitives: &mut Vec<Primitive>, ttl: Duration) {
    let now = Instant::now();
    STORE.lock().submit(source, primitives, ttl, now);
}

/// Empty `source`'s slot.
pub fn clear(source: Source) {
    STORE.lock().clear(source);
}

/// Copy every live slot's primitives into `out` (cleared first), and return each source's live
/// count, indexed by [`Source::index`].
pub fn snapshot(out: &mut Vec<Primitive>) -> [usize; Source::COUNT] {
    let now = Instant::now();
    let (counts, truncated) = STORE.lock().collect(now, out);
    if truncated && !TRUNCATION_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            limit = MAX_PRIMITIVES,
            "overlay: more primitives were submitted than the renderer draws; the excess is dropped"
        );
    }
    counts
}

/// The slots, independent of the clock so that the expiry is testable.
pub struct Store {
    slots: [Slot; Source::COUNT],
}

impl Store {
    pub const fn new() -> Self {
        Self {
            slots: [const { Slot::EMPTY }; Source::COUNT],
        }
    }

    pub fn submit(
        &mut self,
        source: Source,
        primitives: &mut Vec<Primitive>,
        ttl: Duration,
        now: Instant,
    ) {
        let slot = &mut self.slots[source.index()];
        std::mem::swap(&mut slot.primitives, primitives);
        slot.stamped = Some(now);
        slot.ttl = ttl;
    }

    pub fn clear(&mut self, source: Source) {
        let slot = &mut self.slots[source.index()];
        slot.primitives.clear();
        slot.stamped = None;
    }

    /// Copy the live slots' primitives into `out`, at most [`MAX_PRIMITIVES`] of them, in source
    /// order. Returns each source's contribution and whether any were dropped.
    pub fn collect(
        &self,
        now: Instant,
        out: &mut Vec<Primitive>,
    ) -> ([usize; Source::COUNT], bool) {
        out.clear();
        let mut counts = [0; Source::COUNT];
        let mut truncated = false;
        for (slot, count) in self.slots.iter().zip(&mut counts) {
            if !slot.live(now) {
                continue;
            }
            let room = MAX_PRIMITIVES - out.len();
            let taken = slot.primitives.len().min(room);
            truncated |= taken < slot.primitives.len();
            out.extend_from_slice(&slot.primitives[..taken]);
            *count = taken;
        }
        (counts, truncated)
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

static STORE: Mutex<Store> = Mutex::new(Store::new());

static TRUNCATION_LOGGED: AtomicBool = AtomicBool::new(false);

struct Slot {
    primitives: Vec<Primitive>,
    /// When the slot was last submitted, or `None` when it is empty.
    stamped: Option<Instant>,
    ttl: Duration,
}

impl Slot {
    const EMPTY: Slot = Slot {
        primitives: Vec::new(),
        stamped: None,
        ttl: Duration::ZERO,
    };

    fn live(&self, now: Instant) -> bool {
        self.stamped
            .is_some_and(|stamped| now.saturating_duration_since(stamped) <= self.ttl)
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::controllers::overlay::primitive::{Anchor, DepthMode};

    fn line(x: f32) -> Primitive {
        Primitive::Line {
            from: Anchor::World(Vec3::ZERO),
            to: Anchor::World(Vec3::new(x, 0.0, 0.0)),
            width: 0.01,
            color: [1.0; 4],
            depth: DepthMode::Tested,
        }
    }

    const TTL: Duration = Duration::from_millis(100);

    #[test]
    fn a_submission_replaces_the_slot_and_hands_back_the_old_one() {
        let mut store = Store::new();
        let now = Instant::now();
        let mut first = vec![line(1.0), line(2.0)];
        store.submit(Source::GunReadout, &mut first, TTL, now);
        assert!(first.is_empty());

        let mut second = vec![line(3.0)];
        store.submit(Source::GunReadout, &mut second, TTL, now);
        assert_eq!(second, vec![line(1.0), line(2.0)]);

        let mut out = Vec::new();
        let (counts, truncated) = store.collect(now, &mut out);
        assert_eq!(out, vec![line(3.0)]);
        assert_eq!(counts[Source::GunReadout.index()], 1);
        assert!(!truncated);
    }

    #[test]
    fn a_slot_expires_after_its_time_to_live() {
        let mut store = Store::new();
        let now = Instant::now();
        store.submit(Source::GrappleBeam, &mut vec![line(1.0)], TTL, now);
        let mut out = Vec::new();
        store.collect(now + TTL, &mut out);
        assert_eq!(out.len(), 1);
        store.collect(now + TTL + Duration::from_millis(1), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn clearing_empties_only_that_slot() {
        let mut store = Store::new();
        let now = Instant::now();
        store.submit(Source::GrappleBeam, &mut vec![line(1.0)], TTL, now);
        store.submit(Source::DebugGizmos, &mut vec![line(2.0)], TTL, now);
        store.clear(Source::GrappleBeam);
        let mut out = Vec::new();
        let (counts, _) = store.collect(now, &mut out);
        assert_eq!(out, vec![line(2.0)]);
        assert_eq!(counts[Source::GrappleBeam.index()], 0);
        assert_eq!(counts[Source::DebugGizmos.index()], 1);
    }

    #[test]
    fn the_collection_is_capped() {
        let mut store = Store::new();
        let now = Instant::now();
        let mut many = vec![line(1.0); MAX_PRIMITIVES - 1];
        store.submit(Source::GrabMarkers, &mut many, TTL, now);
        store.submit(Source::GhostHands, &mut vec![line(2.0); 5], TTL, now);
        let mut out = Vec::new();
        let (counts, truncated) = store.collect(now, &mut out);
        assert_eq!(out.len(), MAX_PRIMITIVES);
        assert_eq!(counts[Source::GhostHands.index()], 1);
        assert!(truncated);
    }

    #[test]
    fn the_source_indices_match_their_order() {
        for (i, source) in Source::ALL.into_iter().enumerate() {
            assert_eq!(source.index(), i);
        }
    }
}
