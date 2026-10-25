//! The retained per-source slots.
//!
//! Producers run at different rates on different threads: the sim tick, the character workers, the
//! main thread's frame loop, and the render thread. The renderer draws at frame rate. A list
//! cleared every rendered frame would flicker for the slower producers, so each source instead owns
//! a slot that its submission replaces wholesale and stamps. The renderer draws a slot until its
//! time to live runs out, so a producer that stops submitting (its toggle turned off, a mode change,
//! a menu) disappears on its own. Temporary output suppression leaves these submissions and their
//! expiry unchanged.
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

/// Sources whose retained primitives are omitted from renderer snapshots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceMask([bool; Source::COUNT]);

impl SourceMask {
    pub const NONE: Self = Self([false; Source::COUNT]);

    pub fn insert(&mut self, source: Source) {
        self.0[source.index()] = true;
    }

    pub fn contains(self, source: Source) -> bool {
        self.0[source.index()]
    }
}

/// Primitive counts at collection time, indexed by [`Source::index`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Collection {
    /// All live retained primitives, before suppression and the renderer cap.
    pub retained: [usize; Source::COUNT],
    /// Primitives copied to the renderer, after suppression and the cap, before resolution.
    pub visible: [usize; Source::COUNT],
    /// Live primitives omitted by the temporary source mask, before the cap.
    pub suppressed: [usize; Source::COUNT],
    /// Whether the renderer cap drops any unsuppressed primitives.
    pub truncated: bool,
}

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

/// Replace the temporary output mask without changing retained slots or producer state.
/// The visualization controls are the sole runtime writer; producers do not change this mask.
pub fn set_source_mask(mask: SourceMask) {
    STORE.lock().set_source_mask(mask);
}

/// Copy the unsuppressed live primitives into `out` (cleared first), up to the renderer cap.
pub fn snapshot(out: &mut Vec<Primitive>) -> Collection {
    let now = Instant::now();
    let collection = STORE.lock().collect(now, out);
    if collection.truncated && !TRUNCATION_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            limit = MAX_PRIMITIVES,
            "overlay: more primitives were submitted than the renderer draws; the excess is dropped"
        );
    }
    collection
}

/// The slots, independent of the clock so that the expiry is testable.
pub struct Store {
    slots: [Slot; Source::COUNT],
    source_mask: SourceMask,
}

impl Store {
    pub const fn new() -> Self {
        Self {
            slots: [const { Slot::EMPTY }; Source::COUNT],
            source_mask: SourceMask::NONE,
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

    pub fn set_source_mask(&mut self, mask: SourceMask) {
        self.source_mask = mask;
    }

    /// Copy live, unsuppressed primitives in source order, applying suppression before the cap.
    pub fn collect(&self, now: Instant, out: &mut Vec<Primitive>) -> Collection {
        out.clear();
        let mut collection = Collection::default();
        for source in Source::ALL {
            let index = source.index();
            let slot = &self.slots[index];
            if !slot.live(now) {
                continue;
            }
            collection.retained[index] = slot.primitives.len();
            if self.source_mask.contains(source) {
                collection.suppressed[index] = slot.primitives.len();
                continue;
            }
            let room = MAX_PRIMITIVES - out.len();
            let taken = slot.primitives.len().min(room);
            collection.truncated |= taken < slot.primitives.len();
            out.extend_from_slice(&slot.primitives[..taken]);
            collection.visible[index] = taken;
        }
        collection
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
    const REGISTERED_SOURCES: [Source; 8] = [
        Source::BodyMirror,
        Source::DebugGizmos,
        Source::GhostHands,
        Source::GrabMarkers,
        Source::GrappleHighlight,
        Source::GrappleBeam,
        Source::GrabView,
        Source::ColliderView,
    ];

    fn registered_mask() -> SourceMask {
        let mut mask = SourceMask::NONE;
        for source in REGISTERED_SOURCES {
            mask.insert(source);
        }
        mask
    }

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
        let collection = store.collect(now, &mut out);
        assert_eq!(out, vec![line(3.0)]);
        assert_eq!(collection.retained[Source::GunReadout.index()], 1);
        assert_eq!(collection.visible[Source::GunReadout.index()], 1);
        assert!(!collection.truncated);
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
        let collection = store.collect(now, &mut out);
        assert_eq!(out, vec![line(2.0)]);
        assert_eq!(collection.retained[Source::GrappleBeam.index()], 0);
        assert_eq!(collection.visible[Source::DebugGizmos.index()], 1);
    }

    #[test]
    fn the_collection_is_capped_without_losing_retained_counts() {
        let mut store = Store::new();
        let now = Instant::now();
        let mut many = vec![line(1.0); MAX_PRIMITIVES - 1];
        store.submit(Source::GrabMarkers, &mut many, TTL, now);
        store.submit(Source::GhostHands, &mut vec![line(2.0); 5], TTL, now);
        let mut out = Vec::new();
        let collection = store.collect(now, &mut out);
        assert_eq!(out.len(), MAX_PRIMITIVES);
        assert_eq!(collection.retained[Source::GhostHands.index()], 5);
        assert_eq!(collection.visible[Source::GhostHands.index()], 1);
        assert_eq!(collection.suppressed, [0; Source::COUNT]);
        assert!(collection.truncated);
    }

    #[test]
    fn every_registered_source_is_hidden_and_every_other_source_is_unchanged() {
        let mut store = Store::new();
        let now = Instant::now();
        for source in Source::ALL {
            store.submit(source, &mut vec![line(source.index() as f32)], TTL, now);
        }
        let mut out = Vec::new();
        store.collect(now, &mut out);
        let original = out.clone();
        store.set_source_mask(registered_mask());
        let collection = store.collect(now, &mut out);
        let mut expected = Vec::new();
        for source in Source::ALL {
            let hidden = REGISTERED_SOURCES.contains(&source);
            assert_eq!(collection.retained[source.index()], 1);
            assert_eq!(collection.visible[source.index()], usize::from(!hidden));
            assert_eq!(collection.suppressed[source.index()], usize::from(hidden));
            if !hidden {
                expected.push(line(source.index() as f32));
            }
        }
        assert_eq!(out, expected);
        assert!(!collection.truncated);
        store.set_source_mask(SourceMask::NONE);
        let restored = store.collect(now, &mut out);
        assert_eq!(out, original);
        assert_eq!(restored.visible, [1; Source::COUNT]);
        assert_eq!(restored.suppressed, [0; Source::COUNT]);
    }

    #[test]
    fn suppressed_sources_do_not_consume_the_renderer_cap() {
        let mut store = Store::new();
        let now = Instant::now();
        store.submit(
            Source::GrappleHighlight,
            &mut vec![line(1.0); MAX_PRIMITIVES + 1],
            TTL,
            now,
        );
        store.submit(Source::GunReadout, &mut vec![line(2.0); 3], TTL, now);
        store.set_source_mask(registered_mask());
        let mut out = Vec::new();
        let collection = store.collect(now, &mut out);
        assert_eq!(out, vec![line(2.0); 3]);
        assert_eq!(
            collection.retained[Source::GrappleHighlight.index()],
            MAX_PRIMITIVES + 1
        );
        assert_eq!(
            collection.suppressed[Source::GrappleHighlight.index()],
            MAX_PRIMITIVES + 1
        );
        assert_eq!(collection.visible[Source::GunReadout.index()], 3);
        assert!(!collection.truncated);
    }

    #[test]
    fn hidden_slots_accept_refreshes_and_clears() {
        let mut store = Store::new();
        let now = Instant::now();
        store.submit(Source::GrabView, &mut vec![line(1.0)], TTL, now);
        store.set_source_mask(registered_mask());
        let mut replacement = vec![line(2.0)];
        store.submit(Source::GrabView, &mut replacement, TTL, now + TTL / 2);
        assert_eq!(replacement, vec![line(1.0)]);
        let later = now + TTL + Duration::from_millis(1);
        let mut out = Vec::new();
        let hidden = store.collect(later, &mut out);
        assert!(out.is_empty());
        assert_eq!(hidden.retained[Source::GrabView.index()], 1);
        assert_eq!(hidden.suppressed[Source::GrabView.index()], 1);
        store.set_source_mask(SourceMask::NONE);
        store.collect(later, &mut out);
        assert_eq!(out, vec![line(2.0)]);
        store.set_source_mask(registered_mask());
        store.clear(Source::GrabView);
        let cleared = store.collect(later, &mut out);
        assert_eq!(cleared.retained[Source::GrabView.index()], 0);
        assert_eq!(cleared.suppressed[Source::GrabView.index()], 0);
        store.set_source_mask(SourceMask::NONE);
        store.collect(later, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn hidden_slots_expire_normally_and_restore_does_not_revive_them() {
        let mut store = Store::new();
        let now = Instant::now();
        store.submit(Source::BodyMirror, &mut vec![line(1.0)], TTL, now);
        store.set_source_mask(registered_mask());
        let mut out = Vec::new();
        let at_expiry = store.collect(now + TTL, &mut out);
        assert_eq!(at_expiry.suppressed[Source::BodyMirror.index()], 1);
        assert!(out.is_empty());
        let later = now + TTL + Duration::from_millis(1);
        assert_eq!(store.collect(later, &mut out), Collection::default());
        store.set_source_mask(SourceMask::NONE);
        assert_eq!(store.collect(later, &mut out), Collection::default());
        assert!(out.is_empty());
    }

    #[test]
    fn the_source_indices_match_their_order() {
        for (i, source) in Source::ALL.into_iter().enumerate() {
            assert_eq!(source.index(), i);
        }
    }
}
