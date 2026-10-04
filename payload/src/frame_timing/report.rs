//! The plain-number side of the frame timing: the totals drained from the live counters, their
//! accumulation over a reporting window, and the per-frame breakdown the log line and the debug UI
//! show. Nothing here touches the counters or the clock, so it is tested directly.

use std::fmt;

use crate::frame_timing::slot::{Kind, Slot, ThreadClass};

/// One slot's accumulated time over a window.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub(crate) struct SlotTotals {
    /// Time inside the slot's scopes, excluding the scopes nested in them.
    pub(crate) self_ns: u64,
    /// How many scopes of the slot closed.
    pub(crate) calls: u64,
    /// The longest single scope of the slot, nested scopes included.
    pub(crate) max_ns: u64,
}

impl SlotTotals {
    /// No time.
    pub(crate) const ZERO: SlotTotals = SlotTotals {
        self_ns: 0,
        calls: 0,
        max_ns: 0,
    };

    fn absorb(&mut self, other: &SlotTotals) {
        self.self_ns = self.self_ns.saturating_add(other.self_ns);
        self.calls = self.calls.saturating_add(other.calls);
        self.max_ns = self.max_ns.max(other.max_ns);
    }
}

/// Everything the counters accumulated over a window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Totals {
    /// Game frames that ended in the window, as counted by the interval between successive
    /// `Game::Update` entries.
    pub(crate) frames: u64,
    /// The sum of those frame intervals.
    pub(crate) frame_ns: u64,
    /// The longest of those frame intervals.
    pub(crate) frame_max_ns: u64,
    /// Per-slot totals, indexed by [`Slot::index`].
    pub(crate) slots: [SlotTotals; Slot::COUNT],
    /// Self time by the thread it ran on and its kind, indexed by [`ThreadClass::index`] then
    /// [`Kind::index`].
    pub(crate) buckets: [[u64; Kind::COUNT]; ThreadClass::COUNT],
}

impl Totals {
    /// An empty window.
    pub(crate) const EMPTY: Totals = Totals {
        frames: 0,
        frame_ns: 0,
        frame_max_ns: 0,
        slots: [SlotTotals::ZERO; Slot::COUNT],
        buckets: [[0; Kind::COUNT]; ThreadClass::COUNT],
    };

    /// Add another window's totals to this one.
    pub(crate) fn absorb(&mut self, other: &Totals) {
        self.frames = self.frames.saturating_add(other.frames);
        self.frame_ns = self.frame_ns.saturating_add(other.frame_ns);
        self.frame_max_ns = self.frame_max_ns.max(other.frame_max_ns);
        for (mine, theirs) in self.slots.iter_mut().zip(&other.slots) {
            mine.absorb(theirs);
        }
        for (mine, theirs) in self.buckets.iter_mut().zip(&other.buckets) {
            for (mine, theirs) in mine.iter_mut().zip(theirs) {
                *mine = mine.saturating_add(*theirs);
            }
        }
    }

    /// A slot's totals.
    pub(crate) fn slot(&self, slot: Slot) -> &SlotTotals {
        &self.slots[slot.index()]
    }

    /// The self time a thread class spent on one kind of work.
    pub(crate) fn bucket(&self, class: ThreadClass, kind: Kind) -> u64 {
        self.buckets[class.index()][kind.index()]
    }

    /// The game thread's time per frame that no timed scope covers: the frame interval minus every
    /// self time recorded on the game thread. This is the engine's work outside `Game::Update` (the
    /// message pump and the main loop around it) plus timer overhead.
    pub(crate) fn game_untracked_ns(&self) -> u64 {
        let covered = self.buckets[ThreadClass::Game.index()]
            .iter()
            .fold(0u64, |sum, ns| sum.saturating_add(*ns));
        self.frame_ns.saturating_sub(covered)
    }

    /// `ns` as milliseconds per frame over this window, or zero for a window with no frames.
    pub(crate) fn per_frame_ms(&self, ns: u64) -> f64 {
        if self.frames == 0 {
            return 0.0;
        }
        ns as f64 / self.frames as f64 / 1e6
    }

    /// The mean frame interval in milliseconds.
    pub(crate) fn mean_frame_ms(&self) -> f64 {
        self.per_frame_ms(self.frame_ns)
    }
}

/// A finished window, ready to show: its totals and how long it ran.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Report {
    /// The wall-clock length of the window, in seconds.
    pub(crate) window_s: f64,
    pub(crate) totals: Totals,
}

impl fmt::Display for Report {
    /// The one-line breakdown: the frame interval, then per thread the time per frame by kind,
    /// then every slot that ran as `name=<self ms per frame>/<max ms per call>`. All times are
    /// milliseconds.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = &self.totals;
        write!(
            f,
            "frame breakdown window_s={:.1} frames={} frame_ms={:.2} frame_max_ms={:.2}",
            self.window_s,
            t.frames,
            t.mean_frame_ms(),
            ns_to_ms(t.frame_max_ns),
        )?;
        for class in ThreadClass::ALL {
            let total: u64 = Kind::ALL.iter().map(|kind| t.bucket(class, *kind)).sum();
            if total == 0 && class != ThreadClass::Game {
                continue;
            }
            write!(f, " | {}:", class.name())?;
            for kind in Kind::ALL {
                write!(
                    f,
                    " {}={:.2}",
                    kind.name(),
                    t.per_frame_ms(t.bucket(class, kind))
                )?;
            }
            if class == ThreadClass::Game {
                write!(f, " untracked={:.2}", t.per_frame_ms(t.game_untracked_ns()))?;
            }
        }
        write!(f, " | slots:")?;
        for slot in Slot::ALL {
            let s = t.slot(slot);
            if s.calls == 0 {
                continue;
            }
            write!(
                f,
                " {}={:.2}/{:.2}",
                slot.name(),
                t.per_frame_ms(s.self_ns),
                ns_to_ms(s.max_ns),
            )?;
        }
        Ok(())
    }
}

/// Nanoseconds as milliseconds.
pub(crate) fn ns_to_ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    fn totals_with(frames: u64, frame_ms: u64) -> Totals {
        Totals {
            frames,
            frame_ns: frames * frame_ms * MS,
            frame_max_ns: frame_ms * MS,
            ..Totals::default()
        }
    }

    #[test]
    fn absorbing_sums_times_and_keeps_the_larger_maximum() {
        let mut a = totals_with(10, 14);
        a.slots[Slot::DebugUi.index()] = SlotTotals {
            self_ns: 5 * MS,
            calls: 10,
            max_ns: 2 * MS,
        };
        a.buckets[ThreadClass::Game.index()][Kind::Mod.index()] = 5 * MS;

        let mut b = totals_with(5, 28);
        b.slots[Slot::DebugUi.index()] = SlotTotals {
            self_ns: 3 * MS,
            calls: 5,
            max_ns: MS,
        };
        b.buckets[ThreadClass::Game.index()][Kind::Mod.index()] = 3 * MS;

        a.absorb(&b);
        assert_eq!(a.frames, 15);
        assert_eq!(a.frame_ns, (140 + 140) * MS);
        assert_eq!(a.frame_max_ns, 28 * MS);
        assert_eq!(
            *a.slot(Slot::DebugUi),
            SlotTotals {
                self_ns: 8 * MS,
                calls: 15,
                max_ns: 2 * MS,
            }
        );
        assert_eq!(a.bucket(ThreadClass::Game, Kind::Mod), 8 * MS);
    }

    #[test]
    fn absorbing_an_empty_window_changes_nothing() {
        let mut a = totals_with(3, 14);
        a.slots[Slot::Drain.index()].self_ns = 7;
        let before = a.clone();
        a.absorb(&Totals::default());
        assert_eq!(a, before);
    }

    #[test]
    fn untracked_is_the_frame_less_the_game_thread_self_time() {
        let mut t = totals_with(2, 10);
        t.buckets[ThreadClass::Game.index()][Kind::Mod.index()] = 4 * MS;
        t.buckets[ThreadClass::Game.index()][Kind::Wait.index()] = 10 * MS;
        // Other threads' time runs alongside the game thread's and does not cover its frame.
        t.buckets[ThreadClass::Tail.index()][Kind::Wait.index()] = 50 * MS;
        assert_eq!(t.game_untracked_ns(), 6 * MS);
    }

    #[test]
    fn untracked_saturates_when_the_scopes_overrun_the_frames() {
        let mut t = totals_with(1, 10);
        t.buckets[ThreadClass::Game.index()][Kind::Engine.index()] = 12 * MS;
        assert_eq!(t.game_untracked_ns(), 0);
    }

    #[test]
    fn per_frame_times_of_an_empty_window_are_zero() {
        let t = Totals::default();
        assert_eq!(t.per_frame_ms(5 * MS), 0.0);
        assert_eq!(t.mean_frame_ms(), 0.0);
    }

    #[test]
    fn the_line_lists_threads_kinds_and_the_slots_that_ran() {
        let mut t = totals_with(4, 14);
        t.slots[Slot::DebugUi.index()] = SlotTotals {
            self_ns: 4 * MS,
            calls: 4,
            max_ns: 3 * MS / 2,
        };
        t.slots[Slot::VrWaitFrame.index()] = SlotTotals {
            self_ns: 20 * MS,
            calls: 4,
            max_ns: 6 * MS,
        };
        t.buckets[ThreadClass::Game.index()][Kind::Mod.index()] = 4 * MS;
        t.buckets[ThreadClass::Game.index()][Kind::Wait.index()] = 20 * MS;
        let report = Report {
            window_s: 10.0,
            totals: t,
        };
        assert_eq!(
            report.to_string(),
            "frame breakdown window_s=10.0 frames=4 frame_ms=14.00 frame_max_ms=14.00 \
             | game: mod=1.00 openxr=0.00 engine=0.00 wait=5.00 untracked=8.00 \
             | slots: debug_ui=1.00/1.50 vr_wait_frame=5.00/6.00"
        );
    }

    #[test]
    fn other_threads_appear_once_they_have_time() {
        let mut t = totals_with(2, 14);
        t.buckets[ThreadClass::Other.index()][Kind::Mod.index()] = MS;
        t.slots[Slot::PostDraw.index()] = SlotTotals {
            self_ns: MS,
            calls: 4,
            max_ns: MS / 2,
        };
        let line = Report {
            window_s: 1.0,
            totals: t,
        }
        .to_string();
        assert!(line.contains(" | other: mod=0.50 "), "{line}");
        assert!(!line.contains("tail:"), "{line}");
        assert!(line.ends_with("post_draw=0.50/0.50"), "{line}");
    }
}
