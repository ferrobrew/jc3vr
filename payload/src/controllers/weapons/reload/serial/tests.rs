use std::num::NonZeroUsize;

use super::*;
use crate::controllers::{frame::Hand, weapons::reload::loose::Loose};

fn request(generation: u64) -> DetachRequest {
    DetachRequest {
        id: MagazineId {
            weapon: 1,
            gun: 0,
            holder: Hand::Right,
        },
        generation,
        expected: Identity::test(),
    }
}

fn mailbox() -> Mailbox {
    Mailbox {
        installed: true,
        accepting: true,
        ..Mailbox::new()
    }
}

fn retained() -> DetachOutcome {
    DetachOutcome::Retained(Loose::test(
        NonZeroUsize::new(1).unwrap(),
        2,
        NonZeroUsize::new(3).unwrap(),
        4,
        17,
    ))
}

#[test]
fn a_new_mailbox_stays_closed_through_lifecycle_preflight() {
    let mut queue = Mailbox::<()>::new();
    let r = request(1);
    assert!(!queue.can_submit(r));
    assert!(!queue.submit(r));
    assert_eq!(queue.begin(), None);
    assert_eq!(queue.poll(r), None);
    assert!(queue.stop_and_drain());
    assert!(queue.stop_and_drain());
    assert!(!queue.installed);
    assert!(!queue.accepting);
    assert!(!queue.submit(r));
}

#[test]
fn draining_an_installed_callback_never_clears_its_installation() {
    let mut queue = mailbox();
    assert!(!queue.stop_and_drain());
    assert!(queue.installed);
    assert!(!queue.accepting);
    assert!(!queue.stop_and_drain());
    assert!(!queue.submit(request(1)));
}

#[test]
fn draining_executing_work_keeps_the_slot_until_its_completion() {
    let mut queue = mailbox();
    let r = request(1);
    assert!(queue.submit(r));
    assert_eq!(queue.begin(), Some(r));
    queue.installed = false;
    assert!(!queue.stop_and_drain());
    assert!(!queue.authorize(r, true));
    assert!(matches!(
        queue.slot,
        Some(Slot::Executing { canceled: true, .. })
    ));
    queue.complete(r, retained());
    assert_eq!(queue.poll(r), None);
    assert!(queue.stop_and_drain());
    assert!(!queue.submit(request(2)));
}

#[test]
fn a_slot_stays_bounded_through_execution_and_unread_completion() {
    let mut queue = mailbox();
    let r = request(1);
    assert!(queue.submit(r));
    assert!(!queue.submit(request(2)));
    assert_eq!(queue.begin(), Some(r));
    assert_eq!(queue.begin(), None);
    assert!(!queue.submit(request(2)));
    assert!(queue.authorize(r, true));
    queue.complete(r, retained());
    assert!(!queue.submit(request(2)));
    assert_eq!(queue.poll(request(2)), None);
    assert_eq!(queue.poll(r), Some(retained()));
    assert_eq!(queue.poll(r), None);
    assert!(!queue.submit(r));
    assert!(queue.submit(request(2)));
}

#[test]
fn cancel_before_execution_consumes_the_generation_without_an_engine_call() {
    let mut queue = mailbox();
    let r = request(1);
    assert!(queue.submit(r));
    queue.cancel(r);
    assert_eq!(queue.begin(), None);
    assert_eq!(queue.poll(r), None);
    assert!(!queue.submit(r));
}

#[test]
fn cancel_during_execution_keeps_the_slot_until_completion_and_discards_only_the_result() {
    let mut queue = mailbox();
    let r = request(1);
    assert!(queue.submit(r));
    assert_eq!(queue.begin(), Some(r));
    queue.cancel(r);
    assert!(!queue.authorize(r, true));
    assert!(!queue.submit(request(2)));
    // A call already authorized may have returned a physical prop. Completion has no rollback.
    queue.complete(r, retained());
    assert_eq!(queue.poll(r), None);
    assert!(queue.submit(request(2)));
}

#[test]
fn invalid_identity_and_teardown_do_not_authorize_a_call() {
    let mut queue = mailbox();
    let r = request(1);
    assert!(queue.submit(r));
    assert_eq!(queue.begin(), Some(r));
    assert!(!queue.authorize(r, false));
    queue.accepting = false;
    queue.installed = false;
    queue.cancel_all();
    assert!(!queue.authorize(r, true));
    assert!(!queue.submit(request(2)));
    queue.complete(r, retained());
    assert!(queue.slot.is_none());
}

#[test]
fn canceling_an_unread_retained_result_consumes_it_without_rollback() {
    let mut queue = mailbox();
    let r = request(1);
    assert!(queue.submit(r));
    assert_eq!(queue.begin(), Some(r));
    queue.complete(r, retained());
    queue.cancel(r);
    assert_eq!(queue.poll(r), None);
    assert_eq!(queue.poll(r), None);
    assert!(!queue.submit(r));
    assert!(queue.submit(request(2)));
}

#[test]
fn completion_identity_allows_only_the_native_spare_replacement() {
    let before = Identity::test();
    let after = Identity {
        spare: 0,
        spare_control: 0,
        ..before
    };
    assert!(before.same_gun(after));
    assert!(!before.same_gun(Identity {
        world: after.world + 1,
        ..after
    }));
    assert!(!before.same_gun(Identity {
        character_control: after.character_control + 1,
        ..after
    }));
    assert!(!before.same_gun(Identity {
        gun: after.gun + 1,
        ..after
    }));
    assert!(!before.same_gun(Identity {
        tag: after.tag + 1,
        ..after
    }));
}

#[test]
fn the_exact_acquired_spare_is_part_of_request_identity() {
    let r = request(1);
    let mut replaced = r;
    replaced.expected.spare += 1;
    assert_ne!(r, replaced);
    replaced = r;
    replaced.expected.spare_control += 1;
    assert_ne!(r, replaced);
    replaced = r;
    replaced.expected.character_control += 1;
    assert_ne!(r, replaced);
}
