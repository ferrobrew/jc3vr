//! Registered contributions keyed by an enum's discriminant: the hands' stage table and their
//! render-time weapon observers are filled at install by the systems above the hands, and run in
//! the enum's order whatever order the registrations ran in.

use std::sync::OnceLock;

/// One registration per slot, the first one standing.
pub(crate) struct Slots<F: Copy, const N: usize>([OnceLock<F>; N]);

impl<F: Copy, const N: usize> Slots<F, N> {
    pub(crate) const fn new() -> Self {
        Self([const { OnceLock::new() }; N])
    }

    /// Fill slot `index`; a second registration of it is ignored with a warning naming `what`.
    pub(crate) fn set(&self, index: usize, value: F, what: &str) {
        if self.0[index].set(value).is_err() {
            tracing::warn!("hands: a {what} is already registered; the new one is ignored");
        }
    }

    #[inline]
    pub(crate) fn get(&self, index: usize) -> Option<F> {
        self.0[index].get().copied()
    }

    /// Each registered contribution in slot order.
    pub(crate) fn each(&self) -> impl Iterator<Item = F> + '_ {
        self.0.iter().filter_map(|slot| slot.get().copied())
    }

    /// The indices of the slots nobody filled.
    pub(crate) fn missing(&self) -> impl Iterator<Item = usize> + '_ {
        self.0
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.get().is_none())
            .map(|(index, _)| index)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    thread_local! {
        static RAN: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    }

    fn first() {
        RAN.with(|ran| ran.borrow_mut().push(0));
    }

    fn second() {
        RAN.with(|ran| ran.borrow_mut().push(1));
    }

    fn third() {
        RAN.with(|ran| ran.borrow_mut().push(2));
    }

    #[test]
    fn contributions_run_in_slot_order_whatever_the_registration_order() {
        let slots: Slots<fn(), 3> = Slots::new();
        slots.set(2, third, "test");
        slots.set(0, first, "test");
        slots.each().for_each(|run| run());
        assert_eq!(RAN.with(|ran| ran.take()), [0, 2]);
        assert_eq!(slots.missing().collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn the_first_registration_of_a_slot_stands() {
        let slots: Slots<fn(), 2> = Slots::new();
        slots.set(1, second, "test");
        slots.set(1, third, "test");
        slots.each().for_each(|run| run());
        assert_eq!(RAN.with(|ran| ran.take()), [1]);
    }
}
