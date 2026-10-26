use std::{cell::Cell, rc::Rc};

use super::*;

struct Lease(Rc<Cell<usize>>);

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn request(generation: u64) -> DetachRequest {
    DetachRequest {
        id: MagazineId {
            weapon: 1,
            gun: 0,
            holder: crate::controllers::frame::Hand::Right,
        },
        generation,
        expected: Identity::test(),
        kind: DetachKind::Pull,
    }
}

fn queue(released: &Rc<Cell<usize>>) -> Mailbox<Lease> {
    let mut state = Mailbox {
        installed: true,
        accepting: true,
        ..Mailbox::new()
    };
    state.retention = Some(Lease(Rc::clone(released)));
    assert!(state.submit(request(1)));
    state
}

fn retained() -> DetachOutcome {
    DetachOutcome::Retained(crate::controllers::weapons::reload::loose::Loose::test(
        std::num::NonZeroUsize::new(1).unwrap(),
        1,
        std::num::NonZeroUsize::new(2).unwrap(),
        1,
        17,
    ))
}

#[test]
fn canceling_pending_releases_the_unused_reservation_once() {
    let released = Rc::new(Cell::new(0));
    let mut state = queue(&released);
    state.cancel(request(1));
    state.cancel(request(1));
    assert_eq!(released.get(), 1);
    assert!(state.retention.is_none());
    assert!(state.slot.is_none());
}

#[test]
fn executing_cancel_keeps_capacity_until_return_then_discards_only_delivery() {
    let released = Rc::new(Cell::new(0));
    let mut state = queue(&released);
    assert_eq!(state.begin(), Some(request(1)));
    let executing = state.retention.take();
    state.cancel(request(1));
    state.cancel_all();
    assert_eq!(released.get(), 0);
    assert!(!state.can_submit(request(2)));
    state.retention = executing;
    state.complete(request(1), retained());
    assert_eq!(released.get(), 1);
    assert_eq!(state.poll(request(1)), None);
    assert!(state.slot.is_none());
}

#[test]
fn unread_results_keep_the_lease_until_poll_or_cancel_once() {
    for cancel in [false, true] {
        let released = Rc::new(Cell::new(0));
        let mut state = queue(&released);
        assert_eq!(state.begin(), Some(request(1)));
        state.complete(request(1), retained());
        assert_eq!(released.get(), 0);
        assert!(!state.can_submit(request(2)));
        if cancel {
            state.cancel(request(1));
        } else {
            assert_eq!(state.poll(request(1)), Some(retained()));
        }
        assert_eq!(state.poll(request(1)), None);
        state.cancel_all();
        assert_eq!(released.get(), 1);
    }
}
