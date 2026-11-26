//! The log of a debug panel that stays up without either side owning the controls.
//!
//! The panel hands the controls to editing (or back to gameplay) only once every input reads
//! neutral, and the bindings' own policy re-arms its chords on the same proof. A neutral that never
//! comes leaves the panel unusable and its close chord dead, which from the headset looks like a
//! hang. The log names what keeps the wait going, once the wait is longer than a routine release
//! and then at an interval, the same way the masked-hand log (`MaskLog`) does for
//! a masked hand.

use std::time::{Duration, Instant};

/// When a wait is due to be logged.
pub(super) struct CaptureWait {
    since: Option<Instant>,
    /// When the wait is next due to be logged, relative to `since`.
    next_log: Duration,
}

impl CaptureWait {
    pub(super) const fn new() -> Self {
        Self {
            since: None,
            next_log: LOG_DELAY,
        }
    }

    /// Fold in one tick. Returns how long the panel has waited when the wait is due to be logged.
    pub(super) fn step(&mut self, waiting: bool, now: Instant) -> Option<Duration> {
        if !waiting {
            *self = Self::new();
            return None;
        }
        let since = *self.since.get_or_insert(now);
        let waited = now.saturating_duration_since(since);
        if waited < self.next_log {
            return None;
        }
        self.next_log = waited + LOG_INTERVAL;
        Some(waited)
    }
}

/// How long the panel waits before the wait is logged: past a release that takes a moment.
const LOG_DELAY: Duration = Duration::from_secs(2);
/// How often a wait that goes on is logged again.
const LOG_INTERVAL: Duration = Duration::from_secs(10);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_a_long_wait_once_then_at_the_interval() {
        let start = Instant::now();
        let mut wait = CaptureWait::new();
        assert_eq!(wait.step(true, start), None);
        assert_eq!(wait.step(true, start + Duration::from_secs(1)), None);
        assert_eq!(
            wait.step(true, start + Duration::from_secs(2)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(wait.step(true, start + Duration::from_secs(5)), None);
        assert_eq!(
            wait.step(true, start + Duration::from_secs(12)),
            Some(Duration::from_secs(12))
        );
    }

    #[test]
    fn an_ended_wait_starts_over() {
        let start = Instant::now();
        let mut wait = CaptureWait::new();
        wait.step(true, start);
        assert_eq!(wait.step(false, start + Duration::from_secs(3)), None);
        assert_eq!(wait.step(true, start + Duration::from_secs(4)), None);
        assert_eq!(
            wait.step(true, start + Duration::from_secs(6)),
            Some(Duration::from_secs(2))
        );
    }
}
