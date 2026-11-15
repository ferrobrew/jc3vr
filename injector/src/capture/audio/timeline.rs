//! Keeping the loopback audio on the wall clock.
//!
//! The audio goes to ffmpeg as raw samples, which it times by their count, while the video is
//! timed by its frame count against the same clock. So every stretch of time must become samples:
//! WASAPI's loopback delivers no packets at all while nothing plays, and a packet can arrive late
//! when the capture thread is held up. Each packet's capture time places it on the timeline: a
//! packet that starts after the samples written so far is preceded by silence for the gap, and one
//! that starts before them (its time already filled with silence) has its overlap skipped. Small
//! differences within [`Timeline::tolerance`] are left alone, since the capture time and the
//! sample count drift against each other by a few samples. While no packet comes, the timeline is
//! filled with silence up to a little behind the present ([`Timeline::fill_to`]), so a packet that
//! is merely late is not overwritten by silence first.
//!
//! A packet whose time is missing or more than [`Timeline::trust`] away from the samples written
//! is taken as following on directly: a capture time that far off is a broken timestamp, not a
//! real gap, since the fill keeps the timeline within a fraction of a second of the present.

/// Where a packet goes on the timeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Placement {
    /// Silent frames to write before the packet.
    pub(crate) silence: u64,
    /// Frames to skip at the packet's start, which the timeline already holds.
    pub(crate) skip: u64,
}

/// The audio written so far, in frames from the take's start.
#[derive(Debug)]
pub(crate) struct Timeline {
    written: u64,
    tolerance: u64,
    hold_back: u64,
    trust: u64,
}

impl Timeline {
    /// A timeline at `rate` frames a second: a 20 ms tolerance, a 200 ms hold-back for the fill,
    /// and timestamps trusted within two seconds.
    pub(crate) fn new(rate: u32) -> Self {
        let rate = u64::from(rate.max(1));
        Self {
            written: 0,
            tolerance: rate / 50,
            hold_back: rate / 5,
            trust: rate * 2,
        }
    }

    /// Place a packet of `len` frames captured at `start` frames from the take's start, or with no
    /// usable capture time.
    pub(crate) fn place(&mut self, start: Option<u64>, len: u64) -> Placement {
        let placement = match start {
            Some(start) if start.abs_diff(self.written) > self.trust => Placement::default(),
            Some(start) if start > self.written + self.tolerance => Placement {
                silence: start - self.written,
                skip: 0,
            },
            Some(start) if start + self.tolerance < self.written => Placement {
                silence: 0,
                skip: (self.written - start).min(len),
            },
            _ => Placement::default(),
        };
        self.written += placement.silence + (len - placement.skip);
        placement
    }

    /// Silent frames to write so the timeline reaches the hold-back behind `now`, frames from the
    /// take's start.
    pub(crate) fn fill_to(&mut self, now: u64) -> u64 {
        let target = now.saturating_sub(self.hold_back);
        let silence = target.saturating_sub(self.written);
        self.written += silence;
        silence
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const RATE: u32 = 48_000;

    fn placement(silence: u64, skip: u64) -> Placement {
        Placement { silence, skip }
    }

    #[test]
    fn contiguous_packets_and_small_jitter_are_written_as_they_come() {
        let mut timeline = Timeline::new(RATE);
        assert_eq!(timeline.place(Some(0), 480), placement(0, 0));
        assert_eq!(timeline.place(Some(480 + 100), 480), placement(0, 0));
        assert_eq!(timeline.place(Some(960 - 300), 480), placement(0, 0));
        assert_eq!(timeline.place(None, 480), placement(0, 0));
    }

    #[test]
    fn a_gap_is_filled_with_silence_and_an_overlap_skipped() {
        let mut timeline = Timeline::new(RATE);
        timeline.place(Some(0), 480);
        // Nothing played for half a second.
        assert_eq!(timeline.place(Some(24_480), 480), placement(24_000, 0));
        // The idle fill ran ahead of a late packet: its first 2000 frames are already silence.
        assert_eq!(timeline.fill_to(24_960 + 9_600 + 2_000), 2_000);
        assert_eq!(timeline.place(Some(24_960), 4_800), placement(0, 2_000));
        // A packet wholly inside the filled time is skipped entirely.
        let mut timeline = Timeline::new(RATE);
        timeline.fill_to(48_000);
        assert_eq!(timeline.place(Some(10_000), 480), placement(0, 480));
    }

    #[test]
    fn the_fill_holds_back_from_the_present() {
        let mut timeline = Timeline::new(RATE);
        assert_eq!(timeline.fill_to(5_000), 0);
        assert_eq!(timeline.fill_to(48_000), 48_000 - 9_600);
        assert_eq!(timeline.fill_to(48_000), 0);
    }

    #[test]
    fn a_wild_timestamp_is_taken_as_contiguous() {
        let mut timeline = Timeline::new(RATE);
        timeline.place(Some(0), 480);
        assert_eq!(timeline.place(Some(10 * 48_000), 480), placement(0, 0));
        assert_eq!(timeline.place(Some(u64::MAX / 2), 480), placement(0, 0));
    }

    #[test]
    fn the_samples_written_track_the_wall_clock_through_idle_and_bursts() {
        let mut timeline = Timeline::new(RATE);
        let mut written = 0u64;
        // Ten seconds: packets every 10 ms for three seconds, silence for four, then packets again
        // delivered in late bursts of five.
        let mut t = 0u64;
        while t < 10 * 48_000 {
            let playing = !(3 * 48_000..7 * 48_000).contains(&t);
            if playing && (t / 480) % 5 == 4 {
                for k in 0..5 {
                    let start = t - 4 * 480 + k * 480;
                    let p = timeline.place(Some(start), 480);
                    written += p.silence + 480 - p.skip;
                }
            } else if playing && t < 3 * 48_000 {
                let p = timeline.place(Some(t), 480);
                written += p.silence + 480 - p.skip;
            }
            written += timeline.fill_to(t + 480);
            t += 480;
        }
        assert!(
            written.abs_diff(t) <= 48_000 / 5 + 480,
            "{written} against {t}"
        );
    }
}
