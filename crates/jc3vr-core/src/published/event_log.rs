//! [`EventLog`]: the last few events a subsystem saw, for its readout.

/// The last `N` events, oldest overwritten first, in a fixed array: pushing never allocates, and
/// the log is [`Copy`] when the events are, so a readout struct that holds one copies out whole.
/// It also counts every event pushed, kept or not.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct EventLog<T, const N: usize> {
    slots: [Option<T>; N],
    /// The slot the next event goes in; once the log is full, the oldest event's.
    next: usize,
    total: u64,
}

impl<T, const N: usize> EventLog<T, N> {
    pub const fn new() -> Self {
        const { assert!(N > 0, "an event log keeps at least one event") };
        Self {
            slots: [const { None }; N],
            next: 0,
            total: 0,
        }
    }

    /// Add `event`, dropping the oldest when the log is full.
    pub fn push(&mut self, event: T) {
        self.slots[self.next] = Some(event);
        self.next = (self.next + 1) % N;
        self.total = self.total.saturating_add(1);
    }

    /// The kept events, oldest first; `.rev()` gives the newest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        // Before the log fills, the slots from `next` on are empty, so this chain is the kept
        // events in order either way.
        let (newer, older) = self.slots.split_at(self.next);
        older.iter().chain(newer).flatten()
    }

    /// The newest event.
    pub fn latest(&self) -> Option<&T> {
        self.slots[(self.next + N - 1) % N].as_ref()
    }

    /// How many events are kept.
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.latest().is_none()
    }

    /// How many events were ever pushed, including those dropped and cleared.
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// Drop the kept events. The total stays.
    pub fn clear(&mut self) {
        self.slots = [const { None }; N];
        self.next = 0;
    }
}

impl<T, const N: usize> Default for EventLog<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Extend<T> for EventLog<T, N> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, events: I) {
        for event in events {
            self.push(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_keeps_the_newest_events_in_order() {
        let mut log: EventLog<u32, 3> = EventLog::new();
        assert!(log.is_empty());
        assert_eq!(log.latest(), None);
        log.extend([1, 2]);
        assert_eq!(log.iter().copied().collect::<Vec<_>>(), [1, 2]);
        log.extend([3, 4, 5]);
        assert_eq!(log.iter().copied().collect::<Vec<_>>(), [3, 4, 5]);
        assert_eq!(log.iter().rev().copied().collect::<Vec<_>>(), [5, 4, 3]);
        assert_eq!(log.latest(), Some(&5));
        assert_eq!(log.len(), 3);
        assert_eq!(log.total(), 5);
    }

    #[test]
    fn clear_drops_the_events_but_keeps_the_total() {
        let mut log: EventLog<u32, 2> = EventLog::new();
        log.extend([1, 2, 3]);
        log.clear();
        assert!(log.is_empty());
        assert_eq!(log.iter().count(), 0);
        assert_eq!(log.total(), 3);
        log.push(4);
        assert_eq!(log.iter().copied().collect::<Vec<_>>(), [4]);
    }
}
