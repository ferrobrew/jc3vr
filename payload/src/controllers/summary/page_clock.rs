//! Visible-time pagination without a controller chord or hidden input ownership.

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PageTime {
    pub index: usize,
    pub count: usize,
    pub remaining_s: f32,
}

#[derive(Debug, Default)]
pub(crate) struct PageClock {
    revision: Option<u64>,
    elapsed_s: f32,
    looked: bool,
}

impl PageClock {
    pub(crate) const fn new() -> Self {
        Self {
            revision: None,
            elapsed_s: 0.0,
            looked: false,
        }
    }

    /// End the current glance without advancing the visible-time clock.
    pub(crate) fn pause(&mut self) {
        self.looked = false;
    }

    /// Only fully shown time advances the page. A new glance or content revision starts at page one.
    pub(crate) fn step(
        &mut self,
        revision: u64,
        count: usize,
        shown: bool,
        glance: bool,
        dt: f32,
    ) -> PageTime {
        let reset = self.revision != Some(revision) || (glance && !self.looked);
        self.revision = Some(revision);
        self.looked = glance;
        let count = count.max(1);
        if reset || count == 1 {
            self.elapsed_s = 0.0;
        } else if shown && dt.is_finite() && dt > 0.0 {
            self.elapsed_s = (self.elapsed_s + dt) % (DWELL_S * count as f32);
        }
        PageTime {
            index: (self.elapsed_s / DWELL_S) as usize % count,
            count,
            remaining_s: DWELL_S - self.elapsed_s % DWELL_S,
        }
    }
}

/// Each page remains unchanged for a readable interval while the wrist is shown.
pub(crate) const DWELL_S: f32 = 8.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_advance_wrap_pause_and_reset_deterministically() {
        let mut clock = PageClock::new();
        assert_eq!(clock.step(1, 3, true, true, 2.0).index, 0);
        assert_eq!(clock.step(1, 3, true, true, DWELL_S).index, 1);
        assert_eq!(clock.step(1, 3, false, false, 100.0).index, 1);
        assert_eq!(clock.step(1, 3, true, true, 1.0).index, 0);
        assert_eq!(clock.step(1, 3, true, true, DWELL_S * 3.0).index, 0);
        assert_eq!(clock.step(2, 3, true, true, DWELL_S).index, 0);
        assert_eq!(clock.step(2, 1, true, true, DWELL_S).count, 1);
        assert_eq!(clock.step(2, 0, true, true, f32::NAN).index, 0);
    }

    #[test]
    fn hidden_prepare_clears_the_glance_without_advancing_time() {
        let mut clock = PageClock::new();
        clock.step(1, 3, true, true, 0.0);
        assert_eq!(clock.step(1, 3, true, true, DWELL_S).index, 1);
        clock.pause();
        assert_eq!(clock.step(1, 3, false, false, 0.0).index, 1);
        assert_eq!(clock.step(1, 3, true, true, 0.0).index, 0);
    }
}
