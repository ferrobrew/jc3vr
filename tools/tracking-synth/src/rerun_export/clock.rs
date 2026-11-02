//! The wall clock behind the recording's clock.
//!
//! A take's video sidecar stamps each frame's predicted display time with the wall-clock time, so a
//! stretch's frames map to the wall clock and, through the video's start, to the video's clock. A
//! recording without a sidecar is anchored by its first take marker, which carries the wall-clock
//! time it was made at, and runs at real time from there. A recording with neither has no wall
//! clock.

use tracking_format::{Frame, Recording, event::EventKind};

use crate::rig_eval::VideoClock;

/// A map between the recording's clock and the wall clock.
#[derive(Debug, Clone, PartialEq)]
pub struct WallClock {
    /// `(time_ns, unix_ms)`, in time order, with the wall clock never going back.
    points: Vec<(u64, f64)>,
}

impl WallClock {
    /// The map for a stretch's `frames`, from the sidecar or else the recording's first take marker.
    pub fn new(
        recording: &Recording,
        frames: &[Frame],
        sidecar: Option<&VideoClock>,
    ) -> Option<Self> {
        let mut points: Vec<(u64, f64)> = match sidecar {
            Some(clock) => frames
                .iter()
                .filter_map(|frame| Some((frame.time_ns, clock.unix_ms(frame.display_time_ns)?)))
                .collect(),
            None => Vec::new(),
        };
        if points.is_empty() {
            points = recording
                .events
                .iter()
                .find_map(|event| match &event.kind {
                    EventKind::TakeMarker(marker) => Some((event.time_ns, marker.unix_ms as f64)),
                    _ => None,
                })
                .into_iter()
                .collect();
        }
        // The sidecar's stamps are whole milliseconds and a late frame can carry an earlier one;
        // the map keeps the wall clock monotonic so that it inverts.
        let mut latest = f64::NEG_INFINITY;
        for point in &mut points {
            latest = latest.max(point.1);
            point.1 = latest;
        }
        (!points.is_empty()).then_some(Self { points })
    }

    /// The wall-clock time of a recording time, milliseconds since the Unix epoch: interpolated
    /// between the map's points, and at real time past either end.
    pub fn unix_ms(&self, time_ns: u64) -> f64 {
        let index = self.points.partition_point(|p| p.0 < time_ns);
        if index > 0 && index < self.points.len() {
            let ((t0, u0), (t1, u1)) = (self.points[index - 1], self.points[index]);
            return u0 + (u1 - u0) * ((time_ns - t0) as f64 / (t1 - t0) as f64);
        }
        let (t, u) = if index == 0 {
            self.points[0]
        } else {
            self.points[self.points.len() - 1]
        };
        u + (time_ns as f64 - t as f64) / 1.0e6
    }

    /// [`unix_ms`](Self::unix_ms) in nanoseconds, for the timeline.
    pub fn unix_ns(&self, time_ns: u64) -> Option<i64> {
        let ns = self.unix_ms(time_ns) * 1.0e6;
        ns.is_finite().then_some(ns as i64)
    }

    /// The recording time of a wall-clock time: the inverse of [`unix_ms`](Self::unix_ms), and
    /// absent before the recording's clock starts.
    pub fn time_ns(&self, unix_ms: f64) -> Option<u64> {
        let index = self.points.partition_point(|p| p.1 < unix_ms);
        let time = if index > 0 && index < self.points.len() {
            let ((t0, u0), (t1, u1)) = (self.points[index - 1], self.points[index]);
            if u1 == u0 {
                t1 as f64
            } else {
                t0 as f64 + (t1 - t0) as f64 * ((unix_ms - u0) / (u1 - u0))
            }
        } else {
            let (t, u) = if index == 0 {
                self.points[0]
            } else {
                self.points[self.points.len() - 1]
            };
            t as f64 + (unix_ms - u) * 1.0e6
        };
        (time.is_finite() && time >= 0.0).then_some(time.round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(points: Vec<(u64, f64)>) -> WallClock {
        WallClock { points }
    }

    #[test]
    fn the_wall_clock_interpolates_and_inverts() {
        let wall = clock(vec![(1_000_000_000, 5_000.0), (2_000_000_000, 6_010.0)]);
        assert_eq!(wall.unix_ms(1_500_000_000), 5_505.0);
        assert_eq!(wall.time_ns(5_505.0), Some(1_500_000_000));
        // Past either end, real time.
        assert_eq!(wall.unix_ms(3_000_000_000), 7_010.0);
        assert_eq!(wall.unix_ms(0), 4_000.0);
        assert_eq!(wall.time_ns(7_010.0), Some(3_000_000_000));
        assert_eq!(wall.time_ns(4_000.0), Some(0));
        assert_eq!(wall.time_ns(3_000.0), None);
    }

    #[test]
    fn one_anchor_runs_at_real_time() {
        let wall = clock(vec![(2_000_000_000, 10_000.0)]);
        assert_eq!(wall.unix_ms(2_500_000_000), 10_500.0);
        assert_eq!(wall.time_ns(9_000.0), Some(1_000_000_000));
    }
}
