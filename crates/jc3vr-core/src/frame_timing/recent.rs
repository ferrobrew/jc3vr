//! The recent frame intervals and their percentiles: the spread the window averages hide, for the
//! status rows ([`crate::status`]).

/// How many recent intervals are kept: a few seconds at headset rates.
const CAPACITY: usize = 240;

/// The median and the 95th percentile of the recent frame intervals.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct FramePercentiles {
    pub(crate) p50_ms: f32,
    pub(crate) p95_ms: f32,
}

/// The last [`CAPACITY`] frame intervals, in microseconds, oldest overwritten first.
pub(super) struct RecentFrames {
    intervals: [u32; CAPACITY],
    len: usize,
    next: usize,
}

impl RecentFrames {
    pub(super) const fn new() -> Self {
        Self {
            intervals: [0; CAPACITY],
            len: 0,
            next: 0,
        }
    }

    /// Record a frame interval of `ns` nanoseconds.
    pub(super) fn push(&mut self, ns: u64) {
        self.intervals[self.next] = u32::try_from(ns / 1000).unwrap_or(u32::MAX);
        self.next = (self.next + 1) % CAPACITY;
        self.len = (self.len + 1).min(CAPACITY);
    }

    /// The percentiles of the recorded intervals, or `None` before any was recorded. Each is the
    /// nearest-rank percentile: the smallest interval at least that share of the frames fit within.
    pub(super) fn percentiles(&self) -> Option<FramePercentiles> {
        if self.len == 0 {
            return None;
        }
        let mut sorted = self.intervals;
        let sorted = &mut sorted[..self.len];
        sorted.sort_unstable();
        let rank = |percent: usize| {
            let index = (percent * sorted.len()).div_ceil(100).saturating_sub(1);
            sorted[index] as f32 / 1000.0
        };
        Some(FramePercentiles {
            p50_ms: rank(50),
            p95_ms: rank(95),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_recorded_has_no_percentiles() {
        assert_eq!(RecentFrames::new().percentiles(), None);
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let mut recent = RecentFrames::new();
        for ms in 1..=100 {
            recent.push(ms * 1_000_000);
        }
        let p = recent.percentiles().unwrap();
        assert_eq!(p.p50_ms, 50.0);
        assert_eq!(p.p95_ms, 95.0);
    }

    #[test]
    fn a_single_frame_is_every_percentile() {
        let mut recent = RecentFrames::new();
        recent.push(11_100_000);
        let p = recent.percentiles().unwrap();
        assert_eq!(p.p50_ms, 11.1);
        assert_eq!(p.p95_ms, 11.1);
    }

    #[test]
    fn old_intervals_are_overwritten() {
        let mut recent = RecentFrames::new();
        for _ in 0..CAPACITY {
            recent.push(100_000_000);
        }
        for _ in 0..CAPACITY {
            recent.push(10_000_000);
        }
        let p = recent.percentiles().unwrap();
        assert_eq!(p.p50_ms, 10.0);
        assert_eq!(p.p95_ms, 10.0);
    }
}
