//! Controlled transport tests use logical objects, never native pointers or engine allocations.

use std::num::NonZeroU64;

use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};

use super::*;
use crate::controllers::loadout::{
    ObjectAddress, WeaponType,
    lifecycle_ammo::GunAmmo,
    lifecycle_source::{
        ActorKey, Generation, Member, Offering, RequestBook, ReturnedActor, SourceKey,
    },
    spawn_transport::{
        controlled::{Error, Limits, Quiescent, Transport},
        ownership::{
            BorrowedGroup, Callback, CleanupProgress, Engine, Epoch, GroupId, OwnedGroup,
            ReleaseReason, Ticket,
        },
    },
};

#[derive(Debug, PartialEq, Eq)]
struct Strong(usize);

#[derive(Debug, Default)]
struct ControlledEngine {
    spawned: Vec<Ticket>,
    cancelled: Vec<Ticket>,
    cleanup: Vec<(GroupId, usize)>,
    released: Vec<(GroupId, ReleaseReason, usize)>,
    refuse_cleanup: bool,
    wrong_count: bool,
}

impl Engine for ControlledEngine {
    type Strong = Strong;

    fn spawn(&mut self, ticket: Ticket) {
        self.spawned.push(ticket);
    }

    fn cancel(&mut self, ticket: Ticket) {
        self.cancelled.push(ticket);
    }

    fn cleanup_group(&mut self, group: &OwnedGroup<Strong>) -> CleanupProgress {
        self.cleanup.push((group.id, group.objects.len()));
        if self.refuse_cleanup {
            CleanupProgress::Indeterminate
        } else {
            CleanupProgress::Queued {
                objects: group.objects.len() + usize::from(self.wrong_count),
            }
        }
    }

    fn release_group(&mut self, group: OwnedGroup<Strong>, reason: ReleaseReason) {
        self.released.push((group.id, reason, group.objects.len()));
    }
}

fn key(identity: u64) -> SourceKey {
    SourceKey {
        identity: NonZeroU64::new(identity).unwrap(),
        world: Generation(1),
        incarnation: Generation(1),
    }
}

fn weapon() -> WeaponType {
    WeaponType::new(
        17,
        WeaponSlot::E_SLOT_DUAL_WIELD,
        WeaponClass::E_WEAPON_CLASS_PISTOL as i32,
    )
    .unwrap()
}

fn transport(limits: Limits, callback_limit: usize) -> Transport<Strong> {
    let mut book = RequestBook::with_callback_limit(3, 3, callback_limit);
    for identity in 1..=3 {
        book.add_source(
            key(identity),
            Offering::Pair {
                weapon: weapon(),
                ammo: [GunAmmo::new(6, 12), GunAmmo::new(4, 0)],
            },
        )
        .unwrap();
    }
    Transport::controlled(book, limits)
}

fn limits() -> Limits {
    Limits {
        requests: 2,
        groups: 8,
        objects_per_group: 3,
    }
}

fn callback(ticket: Ticket, id: u64, count: usize) -> Callback<Strong> {
    Callback {
        group: OwnedGroup {
            id: GroupId(NonZeroU64::new(id).unwrap()),
            ticket,
            objects: (0..count)
                .map(|index| Strong(id as usize * 10 + index))
                .collect(),
        },
        classified: Some(
            (0..count)
                .map(|index| ReturnedActor {
                    actor: ActorKey {
                        address: ObjectAddress::from_address(id as usize * 10 + index + 1).unwrap(),
                        generation: Generation(1),
                    },
                    weapon: weapon(),
                })
                .collect(),
        ),
    }
}

fn service(transport: &mut Transport<Strong>, engine: &mut ControlledEngine) {
    let epoch = transport.status().epoch;
    let boundary = transport
        .before_pre_update(Quiescent::controlled(epoch), engine)
        .unwrap();
    transport.after_pre_update(boundary, engine).unwrap();
}

#[test]
fn production_gate_is_closed_without_intercepting_normal_gameplay() {
    let mut engine = ControlledEngine::default();
    let gate = controller();
    assert_eq!(gate.status().admission, Admission::Closed);
    assert!(!gate.status().native_callbacks);
    assert!(!gate.status().source_qualification);
    assert!(!gate.status().ammunition);
    assert!(!gate.status().staging);
    assert!(gate.request().is_err());
    let ordinary_gameplay = &mut engine;
    assert!(ordinary_gameplay.spawned.is_empty());
    assert!(ordinary_gameplay.cancelled.is_empty());
    assert!(ordinary_gameplay.cleanup.is_empty());
}

#[test]
fn queued_cancellation_never_submits_and_never_reuses_request_identity() {
    let mut transport = transport(limits(), 8);
    let mut engine = ControlledEngine::default();
    let first = transport.begin(key(1), Member::First).unwrap();
    transport.cancel(first).unwrap();
    assert_eq!(transport.status().request_book.outstanding_requests, 0);
    let second = transport.begin(key(1), Member::First).unwrap();
    assert_ne!(first.request, second.request);
    transport.cancel(second).unwrap();
    service(&mut transport, &mut engine);
    assert!(engine.spawned.is_empty());
    assert!(engine.cancelled.is_empty());
}

#[test]
fn cancellation_acknowledges_only_after_its_later_service_returns() {
    let mut transport = transport(limits(), 8);
    let mut engine = ControlledEngine::default();
    let ticket = transport.begin(key(1), Member::First).unwrap();
    service(&mut transport, &mut engine);
    transport.cancel(ticket).unwrap();
    assert_eq!(transport.status().request_book.cancelled_requests, 1);
    let boundary = transport
        .before_pre_update(Quiescent::controlled(Epoch(1)), &mut engine)
        .unwrap();
    assert_eq!(engine.cancelled, vec![ticket]);
    assert_eq!(transport.status().request_book.outstanding_requests, 1);
    transport.after_pre_update(boundary, &mut engine).unwrap();
    assert_eq!(transport.status().request_book.outstanding_requests, 0);
}

#[test]
fn request_capacity_remains_reserved_until_native_cancellation_boundary() {
    let mut transport = transport(
        Limits {
            requests: 1,
            ..limits()
        },
        8,
    );
    let mut engine = ControlledEngine::default();
    let ticket = transport.begin(key(1), Member::First).unwrap();
    service(&mut transport, &mut engine);
    transport.cancel(ticket).unwrap();
    assert_eq!(transport.begin(key(2), Member::First), Err(Error::Capacity));
    service(&mut transport, &mut engine);
    let next = transport.begin(key(2), Member::First).unwrap();
    assert_ne!(ticket.request, next.request);
}

#[test]
fn complete_group_cleanup_uses_g05_receipt_and_releases_after_service() {
    let mut transport = transport(limits(), 8);
    let mut engine = ControlledEngine::default();
    let ticket = transport.begin(key(1), Member::First).unwrap();
    service(&mut transport, &mut engine);
    transport.callback(callback(ticket, 1, 2)).unwrap();
    assert_eq!(transport.status().request_book.pending_disposals, 2);
    assert_eq!(transport.status().request_book.staged, 0);
    let boundary = transport
        .before_pre_update(Quiescent::controlled(Epoch(1)), &mut engine)
        .unwrap();
    assert_eq!(engine.cleanup.len(), 1);
    assert_eq!(engine.cleanup[0].1, 2);
    assert!(engine.released.is_empty());
    assert_eq!(transport.status().request_book.outstanding_requests, 1);
    transport.after_pre_update(boundary, &mut engine).unwrap();
    assert_eq!(engine.released[0].2, 2);
    assert_eq!(transport.status().request_book.outstanding_requests, 0);
}

#[test]
fn callback_during_service_waits_for_the_next_cleanup_boundary() {
    let mut transport = transport(limits(), 8);
    let mut engine = ControlledEngine::default();
    let ticket = transport.begin(key(1), Member::First).unwrap();
    let boundary = transport
        .before_pre_update(Quiescent::controlled(Epoch(1)), &mut engine)
        .unwrap();
    transport.callback(callback(ticket, 1, 1)).unwrap();
    transport.after_pre_update(boundary, &mut engine).unwrap();
    assert!(engine.cleanup.is_empty());
    assert_eq!(transport.status().retained_groups, 1);
    service(&mut transport, &mut engine);
    assert_eq!(transport.status().retained_groups, 0);
}

#[test]
fn late_duplicate_callbacks_release_references_without_a_second_despawn() {
    let mut transport = transport(limits(), 8);
    let mut engine = ControlledEngine::default();
    let ticket = transport.begin(key(1), Member::First).unwrap();
    service(&mut transport, &mut engine);
    transport.callback(callback(ticket, 1, 2)).unwrap();
    transport.callback(callback(ticket, 1, 2)).unwrap();
    service(&mut transport, &mut engine);
    transport.callback(callback(ticket, 1, 2)).unwrap();
    service(&mut transport, &mut engine);
    assert_eq!(engine.cleanup.len(), 1);
    assert_eq!(engine.released.len(), 3);
    assert_eq!(engine.released[1].1, ReleaseReason::DuplicateReference);
    assert_eq!(engine.released[2].1, ReleaseReason::DuplicateReference);
    assert_eq!(transport.status().request_book.callback_batches, 0);
}

#[test]
fn zero_despawn_or_wrong_group_count_keeps_owned_handles_and_blocks_drain() {
    for wrong_count in [false, true] {
        let mut transport = transport(limits(), 8);
        let mut engine = ControlledEngine {
            refuse_cleanup: !wrong_count,
            wrong_count,
            ..Default::default()
        };
        let ticket = transport.begin(key(1), Member::First).unwrap();
        service(&mut transport, &mut engine);
        transport.callback(callback(ticket, 1, 2)).unwrap();
        service(&mut transport, &mut engine);
        service(&mut transport, &mut engine);
        assert!(engine.released.is_empty());
        assert_eq!(engine.cleanup.len(), 1);
        assert_eq!(transport.status().retained_groups, 1);
        assert_eq!(transport.status().unresolved_groups, 1);
        assert!(!transport.drained());
        assert!(transport.status().closed);
    }
}

#[test]
fn callback_capacity_overflow_returns_owned_whole_group_for_explicit_cleanup() {
    let mut transport = transport(
        Limits {
            groups: 1,
            ..limits()
        },
        8,
    );
    let mut engine = ControlledEngine::default();
    let ticket = transport.begin(key(1), Member::First).unwrap();
    service(&mut transport, &mut engine);
    transport.callback(callback(ticket, 1, 1)).unwrap();
    let refusal = transport.callback(callback(ticket, 2, 3)).unwrap_err();
    assert_eq!(refusal.reason, Error::Capacity);
    assert_eq!(refusal.callback.group.objects.len(), 3);
    assert_eq!(transport.status().group_records, 1);
    assert!(transport.status().closed);
    let boundary = transport
        .before_pre_update(Quiescent::controlled(Epoch(1)), &mut engine)
        .unwrap();
    let overflow = refusal.queue_cleanup(&boundary, &mut engine).unwrap();
    assert!(engine.released.is_empty());
    let returned = transport.after_pre_update(boundary, &mut engine).unwrap();
    overflow.after_pre_update(&returned, &mut engine).unwrap();
    assert_eq!(engine.cleanup.len(), 2);
    assert_eq!(engine.released[1].2, 3);
    assert!(!transport.drained());
}

#[test]
fn oversized_or_g05_saturated_callback_keeps_the_whole_group_without_staging() {
    for callback_limit in [1, 8] {
        let mut transport = transport(
            Limits {
                objects_per_group: 1,
                ..limits()
            },
            callback_limit,
        );
        let mut engine = ControlledEngine::default();
        let ticket = transport.begin(key(1), Member::First).unwrap();
        service(&mut transport, &mut engine);
        transport.callback(callback(ticket, 1, 3)).unwrap();
        assert!(transport.status().faulted);
        assert_eq!(transport.status().request_book.staged, 0);
        service(&mut transport, &mut engine);
        assert_eq!(engine.cleanup[0].1, 3);
        assert_eq!(engine.released[0].2, 3);
        assert!(!transport.drained());
    }
}

#[test]
fn reset_closes_before_late_return_and_keeps_nonreused_ids_and_cleanup_alive() {
    let mut transport = transport(limits(), 8);
    let mut engine = ControlledEngine::default();
    let ticket = transport.begin(key(1), Member::First).unwrap();
    service(&mut transport, &mut engine);
    transport.reset().unwrap();
    assert_eq!(transport.status().epoch, Epoch(2));
    assert_eq!(transport.begin(key(2), Member::First), Err(Error::Closed));
    transport.callback(callback(ticket, 1, 1)).unwrap();
    assert_eq!(transport.status().request_book.staged, 0);
    service(&mut transport, &mut engine);
    assert!(transport.drained());
}

#[test]
fn close_cancels_pending_requests_and_drains_without_changing_gameplay_ownership() {
    let mut transport = transport(limits(), 8);
    let mut engine = ControlledEngine::default();
    transport.begin(key(1), Member::First).unwrap();
    service(&mut transport, &mut engine);
    transport.begin(key(2), Member::First).unwrap();
    transport.close();
    assert!(!transport.drained());
    service(&mut transport, &mut engine);
    assert!(transport.drained());
    assert_eq!(engine.spawned.len(), 1);
    assert_eq!(engine.cancelled.len(), 1);
    assert!(engine.cleanup.is_empty());
}

#[test]
fn reset_during_service_preserves_submitted_cancellation_acknowledgment() {
    let mut transport = transport(limits(), 8);
    let mut engine = ControlledEngine::default();
    let ticket = transport.begin(key(1), Member::First).unwrap();
    service(&mut transport, &mut engine);
    transport.cancel(ticket).unwrap();
    let boundary = transport
        .before_pre_update(Quiescent::controlled(Epoch(1)), &mut engine)
        .unwrap();
    transport.reset().unwrap();
    transport.after_pre_update(boundary, &mut engine).unwrap();
    assert_eq!(transport.status().request_book.outstanding_requests, 0);
    assert!(transport.drained());
}

#[derive(Debug)]
struct Borrowed {
    ticket: Ticket,
    count: usize,
}

impl BorrowedGroup for Borrowed {
    type Strong = Strong;

    fn retain_all(&self) -> OwnedGroup<Strong> {
        callback(self.ticket, 1, self.count).group
    }

    fn classify(&self) -> Option<Vec<ReturnedActor>> {
        None
    }
}

#[test]
fn unclassified_nonweapon_returns_retain_and_cleanup_every_object() {
    let mut transport = transport(limits(), 8);
    let mut engine = ControlledEngine::default();
    let ticket = transport.begin(key(1), Member::First).unwrap();
    service(&mut transport, &mut engine);
    let borrowed = Borrowed { ticket, count: 3 };
    let owned = Callback::retain(&borrowed);
    assert_eq!(owned.group.objects.len(), 3);
    transport.callback(owned).unwrap();
    service(&mut transport, &mut engine);
    assert_eq!(engine.cleanup[0].1, 3);
    assert_eq!(engine.released[0].2, 3);
    assert_eq!(transport.status().request_book.outstanding_requests, 0);
}
