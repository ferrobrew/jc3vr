//! Which recorded frames a replay publishes on each rendered frame.
//!
//! The replay's frame rate is the game's, which need not be the recording's: without a headset the
//! desktop renders at whatever rate it manages. A rendered frame publishes every recorded frame
//! that has come due since the last one, in order, so that a button pressed and released between
//! two rendered frames still reaches the input latch, which folds each publish's edges in. The
//! last one published is what the frame shows.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// How a replay walks through its recording.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Pacing {
    /// At the recording's own clock: a rendered frame publishes every recorded frame whose time
    /// has passed since the replay started.
    #[default]
    RealTime,
    /// One recorded frame per rendered frame, however long the frames take. Repeatable against
    /// the game's own frame count rather than the wall clock, and slower or faster than the
    /// recording when the game renders at another rate.
    FrameLocked,
}

impl Pacing {
    pub(crate) const ALL: [Pacing; 2] = [Pacing::RealTime, Pacing::FrameLocked];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Pacing::RealTime => "real time",
            Pacing::FrameLocked => "one per frame",
        }
    }
}

/// A replay's position in its recording.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Cursor {
    /// The first frame not yet published.
    next: usize,
}

impl Cursor {
    /// The frames to publish now, given each recorded frame's time (nanoseconds since the
    /// recording started, non-decreasing) and the time since the replay started. Moves past them.
    pub(crate) fn advance(
        &mut self,
        times: &[u64],
        elapsed_ns: u64,
        pacing: Pacing,
    ) -> Range<usize> {
        let start = self.next.min(times.len());
        let end = match pacing {
            Pacing::FrameLocked => (start + 1).min(times.len()),
            Pacing::RealTime => {
                let origin = times.first().copied().unwrap_or(0);
                let due = |time: &u64| time.saturating_sub(origin) <= elapsed_ns;
                start + times[start..].iter().take_while(|t| due(t)).count()
            }
        };
        self.next = end;
        start..end
    }

    /// The first frame not yet published.
    pub(crate) fn position(&self) -> usize {
        self.next
    }

    /// Whether every frame of a recording of `len` frames has been published.
    pub(crate) fn finished(&self, len: usize) -> bool {
        self.next >= len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn real_time_publishes_every_frame_that_came_due() {
        let times = [5 * MS, 16 * MS, 27 * MS, 38 * MS];
        let mut cursor = Cursor::default();
        assert_eq!(cursor.advance(&times, 0, Pacing::RealTime), 0..1);
        assert_eq!(cursor.advance(&times, 10 * MS, Pacing::RealTime), 1..1);
        assert_eq!(cursor.advance(&times, 25 * MS, Pacing::RealTime), 1..3);
        assert!(!cursor.finished(times.len()));
        assert_eq!(cursor.advance(&times, 500 * MS, Pacing::RealTime), 3..4);
        assert!(cursor.finished(times.len()));
        assert_eq!(cursor.advance(&times, 600 * MS, Pacing::RealTime), 4..4);
    }

    #[test]
    fn frame_locked_publishes_one_frame_each_call() {
        let times = [0, 100 * MS, 200 * MS];
        let mut cursor = Cursor::default();
        for i in 0..3 {
            assert_eq!(cursor.advance(&times, 0, Pacing::FrameLocked), i..i + 1);
        }
        assert!(cursor.finished(times.len()));
        assert_eq!(cursor.advance(&times, 0, Pacing::FrameLocked), 3..3);
    }

    #[test]
    fn an_empty_recording_is_finished_at_once() {
        let mut cursor = Cursor::default();
        assert!(cursor.finished(0));
        assert_eq!(cursor.advance(&[], 0, Pacing::RealTime), 0..0);
    }

    #[test]
    fn frames_with_the_same_time_come_due_together() {
        let times = [0, 0, 10 * MS];
        let mut cursor = Cursor::default();
        assert_eq!(cursor.advance(&times, 0, Pacing::RealTime), 0..2);
    }
}
