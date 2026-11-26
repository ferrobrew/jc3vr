//! Source identity for the staging ring. A source's record advances only when its copy is
//! submitted, so the pixels a map returns always come back with the record they were made from.

use crate::recorder::convert::RING_DEPTH;

pub(crate) struct SourceRing<T> {
    sources: [Option<T>; RING_DEPTH],
    next: usize,
    copies: usize,
}

impl<T: Copy> SourceRing<T> {
    pub(crate) fn new() -> Self {
        Self {
            sources: [None; RING_DEPTH],
            next: 0,
            copies: 0,
        }
    }

    /// The slot the next copy goes into.
    pub(crate) fn copy_slot(&self) -> usize {
        self.next
    }

    /// Record that `source` was copied into [`Self::copy_slot`], and return the oldest slot and its
    /// source once the ring is primed.
    pub(crate) fn copied(&mut self, source: T) -> Option<(usize, T)> {
        self.sources[self.next] = Some(source);
        self.next = (self.next + 1) % RING_DEPTH;
        self.copies = (self.copies + 1).min(RING_DEPTH);
        (self.copies == RING_DEPTH).then(|| {
            (
                self.next,
                self.sources[self.next].expect("recorder: a primed ring slot has its source"),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_pixels_keep_their_source_through_priming_not_ready_and_copy_failure() {
        let mut ring = SourceRing::new();
        let mut pixels = [0; RING_DEPTH];
        let mut last: Option<(u32, u32)> = None;
        for frame in 0..10u32 {
            let copy_slot = ring.copy_slot();
            if frame == 4 {
                // A failure before the copy must not advance the pixels or the source record.
                assert_eq!(copy_slot, ring.copy_slot());
                continue;
            }
            pixels[copy_slot] = frame;
            let read = ring.copied(frame);
            if frame < 2 {
                assert!(read.is_none());
                assert!(last.is_none());
                continue;
            }
            let (slot, source) = read.expect("a primed ring");
            if frame == 3 || frame == 7 {
                // A refused map repeats the last delivered source, not the current one.
                let (pixel, repeated) = last.expect("a prior successful map");
                assert_eq!(pixel, repeated);
                assert_ne!(repeated, frame);
                continue;
            }
            assert_eq!(pixels[slot], source);
            last = Some((pixels[slot], source));
        }
        assert_eq!(last.expect("a delivered frame").0, 7);
    }
}
