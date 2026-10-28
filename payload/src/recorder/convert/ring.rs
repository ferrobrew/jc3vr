//! Source identity for the staging ring. Metadata advances only when its copy is submitted.

use crate::recorder::convert::RING_DEPTH;

pub(super) struct SourceRing<T> {
    sources: [Option<T>; RING_DEPTH],
    next: usize,
    copies: usize,
}

impl<T: Copy> SourceRing<T> {
    pub(super) fn new() -> Self {
        Self {
            sources: [None; RING_DEPTH],
            next: 0,
            copies: 0,
        }
    }

    pub(super) fn copy_slot(&self) -> usize {
        self.next
    }

    pub(super) fn copied(&mut self, source: T) -> Option<(usize, T)> {
        self.sources[self.next] = Some(source);
        self.next = (self.next + 1) % RING_DEPTH;
        self.copies = (self.copies + 1).min(RING_DEPTH);
        (self.copies == RING_DEPTH).then(|| (self.next, self.sources[self.next].unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        recorder::stream::FrameMetadata,
        vr::{CaptureEyeView, CaptureViews},
    };

    #[test]
    fn delayed_pixels_keep_original_pose_through_priming_not_ready_and_copy_failure() {
        let mut ring = SourceRing::new();
        let mut pixels = [0; RING_DEPTH];
        let mut last: Option<(u32, FrameMetadata)> = None;
        for frame in 0..10 {
            let copy_slot = ring.copy_slot();
            if frame == 4 {
                // A failure before CopyResource must not advance pixels or source metadata.
                assert_eq!(copy_slot, ring.copy_slot());
                continue;
            }
            pixels[copy_slot] = frame;
            let read = ring.copied(metadata(frame));
            if frame < 2 {
                assert!(read.is_none());
                assert!(last.is_none());
                continue;
            }
            let (slot, source) = read.unwrap();
            if frame == 3 || frame == 7 {
                // A refused map repeats the last delivered source, not the current source or slot.
                let (pixel, duplicate) = last.expect("a prior successful map");
                assert_source(pixel, duplicate);
                assert_ne!(duplicate.frame, frame);
                continue;
            }
            assert_source(pixels[slot], source);
            last = Some((pixels[slot], source));
        }
        assert_eq!(last.unwrap().0, 7);
    }

    fn metadata(frame: u32) -> FrameMetadata {
        FrameMetadata {
            frame,
            unix_ms: i64::from(frame) * 10,
            game_run_time_us: Some(u64::from(frame) * 100),
            eye: 0,
            width: 2,
            height: 2,
            views: Some(CaptureViews {
                predicted_display_time_ns: i64::from(frame) * 1000,
                eyes: [CaptureEyeView {
                    position: [frame as f32, 1.0, 2.0],
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    fov: [-0.7, 0.7, 0.7, -0.7],
                }; 2],
            }),
        }
    }

    fn assert_source(pixel: u32, source: FrameMetadata) {
        assert_eq!(source.frame, pixel);
        assert_eq!(source.unix_ms, i64::from(pixel) * 10);
        assert_eq!(source.game_run_time_us, Some(u64::from(pixel) * 100));
        let views = source.views.unwrap();
        assert_eq!(views.predicted_display_time_ns, i64::from(pixel) * 1000);
        assert_eq!(views.eyes[0].position[0], pixel as f32);
    }
}
