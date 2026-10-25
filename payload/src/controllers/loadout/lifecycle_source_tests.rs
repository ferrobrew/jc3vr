//! Synthetic source/request fixtures. Actor keys represent no allocated engine objects.

use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};

use super::*;
use crate::controllers::loadout::{
    BodyLayout, ItemKind, Place, Socket,
    table::Loadout,
    transition::{Take, TakeSource},
};

fn key(identity: u64, incarnation: u64) -> SourceKey {
    SourceKey {
        identity: NonZeroU64::new(identity).unwrap(),
        world: Generation(1),
        incarnation: Generation(incarnation),
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

fn actor(address: usize, generation: u64) -> ActorKey {
    ActorKey {
        address: ObjectAddress::from_address(address).unwrap(),
        generation: Generation(generation),
    }
}

fn returned(address: usize) -> ReturnedActor {
    ReturnedActor {
        actor: actor(address, 1),
        weapon: weapon(),
    }
}

fn table() -> Loadout {
    Loadout::new(BodyLayout::full(Hand::Right))
}

fn pair() -> Offering {
    Offering::Pair {
        weapon: weapon(),
        ammo: [GunAmmo::new(6, 12), GunAmmo::new(4, 0)],
    }
}

fn stage(book: &mut RequestBook, source: SourceKey, member: Member, address: usize) {
    let request = book.begin(source, member).unwrap();
    let completion = book.complete(request, vec![returned(address)], &[]);
    assert_eq!(
        completion.dispositions(),
        &[Disposition::Staged(actor(address, 1))]
    );
    book.acknowledge(completion).unwrap();
}

#[test]
fn unlimited_grenade_draws_have_no_stock_or_prior_draw_limit() {
    let mut book = RequestBook::new(1, 0);
    let source = key(1, 1);
    book.add_source(source, Offering::UnlimitedGrenades { kind: 9 })
        .unwrap();
    let snapshot = table().snapshot();
    for _ in 0..1000 {
        let draw = book.draw_safe(source, Hand::Left, &snapshot).unwrap();
        assert_eq!((draw.source, draw.kind, draw.hand), (source, 9, Hand::Left));
    }
    assert_eq!(book.counts().outstanding_requests, 0);
    assert_eq!(book.counts().sources, 1);
    assert_eq!(
        book.begin(source, Member::First),
        Err(SourceError::NoSuchMember)
    );
}

#[test]
fn pair_stages_with_full_carry_space_and_taking_one_leaves_the_other() {
    let mut book = RequestBook::new(1, 2);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let mut carried = table();
    carried
        .take(Take {
            hand: Hand::Left,
            object: actor(99, 1).address,
            kind: ItemKind::Weapon(weapon()),
            source: TakeSource::World,
        })
        .unwrap();
    let mut snapshot = carried.snapshot();
    let mut placeholder = snapshot.items[0];
    for socket in Socket::ALL {
        placeholder.place = Place::Socket(socket);
        snapshot.items.push(placeholder);
    }
    stage(&mut book, source, Member::First, 1);
    stage(&mut book, source, Member::Second, 2);
    let first = book
        .claim(source, Member::First, Hand::Right, &snapshot, Ok)
        .unwrap();
    assert_eq!(first.actor, actor(1, 1));
    assert_eq!(book.member(source, Member::First), Ok(MemberStatus::Taken));
    assert_eq!(
        book.member(source, Member::Second),
        Ok(MemberStatus::Staged(actor(2, 1)))
    );
    assert_eq!(
        book.claim(source, Member::Second, Hand::Left, &snapshot, Ok),
        Err(SourceError::HandOccupied)
    );
    let second = book
        .claim(source, Member::Second, Hand::Right, &snapshot, Ok)
        .unwrap();
    assert_eq!(first.ammo.total() + second.ammo.total(), 22);
    assert_eq!(
        book.begin(source, Member::First),
        Err(SourceError::MemberUnavailable)
    );
}

#[test]
fn ordinary_single_source_has_no_second_member() {
    let source = key(1, 1);
    let mut book = RequestBook::new(1, 2);
    book.add_source(
        source,
        Offering::Single {
            weapon: weapon(),
            ammo: GunAmmo::new(6, 12),
        },
    )
    .unwrap();
    assert_eq!(
        book.begin(source, Member::Second),
        Err(SourceError::NoSuchMember)
    );
    assert_eq!(
        book.member(source, Member::Second),
        Err(SourceError::NoSuchMember)
    );
}

#[test]
fn equal_tag_sources_and_reordered_callbacks_keep_their_own_members() {
    let mut book = RequestBook::new(2, 4);
    let first = key(1, 1);
    let second = key(2, 1);
    book.add_source(first, pair()).unwrap();
    book.add_source(second, pair()).unwrap();
    let a = book.begin(first, Member::First).unwrap();
    let b = book.begin(second, Member::First).unwrap();
    assert_ne!(a, b);
    let done_b = book.complete(b, vec![returned(2)], &[]);
    book.acknowledge(done_b).unwrap();
    let done_a = book.complete(a, vec![returned(1)], &[]);
    book.acknowledge(done_a).unwrap();
    assert_eq!(
        book.member(first, Member::First),
        Ok(MemberStatus::Staged(actor(1, 1)))
    );
    assert_eq!(
        book.member(second, Member::First),
        Ok(MemberStatus::Staged(actor(2, 1)))
    );
}

#[test]
fn timeout_retains_request_capacity_until_late_return_cleanup() {
    let mut book = RequestBook::new(1, 1);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let request = book.begin(source, Member::First).unwrap();
    book.cancel(request).unwrap();
    book.cancel(request).unwrap();
    assert_eq!(book.counts().cancelled_requests, 1);
    assert_eq!(
        book.begin(source, Member::First),
        Err(SourceError::MemberUnavailable)
    );
    assert_eq!(
        book.begin(source, Member::Second),
        Err(SourceError::Capacity)
    );
    let completion = book.complete(request, vec![returned(1)], &[]);
    assert_eq!(
        completion.dispositions(),
        &[Disposition::Dispose(actor(1, 1))]
    );
    assert_eq!(book.counts().awaiting_cleanup, 1);
    assert_eq!(
        book.begin(source, Member::Second),
        Err(SourceError::Capacity)
    );
    book.acknowledge(completion).unwrap();
    let retry = book.begin(source, Member::First).unwrap();
    assert_ne!(retry, request);
}

#[test]
fn multi_object_and_repeated_returns_are_accounted_without_partial_staging() {
    let mut book = RequestBook::new(1, 1);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let request = book.begin(source, Member::First).unwrap();
    let completion = book.complete(request, vec![returned(1), returned(2), returned(1)], &[]);
    assert_eq!(
        completion.dispositions(),
        &[
            Disposition::Dispose(actor(1, 1)),
            Disposition::Dispose(actor(2, 1)),
            Disposition::Repeated(actor(1, 1)),
        ]
    );
    assert_eq!(
        book.member(source, Member::First),
        Ok(MemberStatus::Pending(request))
    );
    book.acknowledge(completion).unwrap();
    assert_eq!(
        book.member(source, Member::First),
        Ok(MemberStatus::Available)
    );
}

#[test]
fn duplicate_callback_never_disposes_an_adopted_actor() {
    let mut book = RequestBook::new(1, 2);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let request = book.begin(source, Member::First).unwrap();
    let completion = book.complete(request, vec![returned(1)], &[]);
    assert_eq!(
        book.begin(source, Member::Second),
        Err(SourceError::SourceBusy)
    );
    let duplicate = book.complete(request, vec![returned(1)], &[]);
    assert_eq!(
        duplicate.dispositions(),
        &[Disposition::AlreadyOwned(actor(1, 1))]
    );
    book.acknowledge(duplicate).unwrap();
    assert_eq!(book.counts().outstanding_requests, 1);
    book.acknowledge(completion).unwrap();
    let claim = book
        .claim(source, Member::First, Hand::Right, &table().snapshot(), Ok)
        .unwrap();
    let duplicate = book.complete(request, vec![returned(1)], &[claim.actor]);
    assert_eq!(
        duplicate.dispositions(),
        &[Disposition::AlreadyOwned(claim.actor)]
    );
    book.acknowledge(duplicate).unwrap();
}

#[test]
fn source_reuse_and_old_world_callbacks_do_not_bind_new_members() {
    let old = key(1, 1);
    let new = SourceKey {
        world: Generation(2),
        ..key(1, 2)
    };
    let mut book = RequestBook::new(2, 2);
    book.add_source(old, pair()).unwrap();
    let pending_old = book.begin(old, Member::First).unwrap();
    book.invalidate(old).unwrap();
    assert_eq!(book.forget(old), Err(SourceError::CleanupOutstanding));
    book.add_source(new, pair()).unwrap();
    let pending_new = book.begin(new, Member::First).unwrap();
    let late = book.complete(pending_old, vec![returned(1)], &[]);
    assert_eq!(late.dispositions(), &[Disposition::Dispose(actor(1, 1))]);
    book.acknowledge(late).unwrap();
    book.forget(old).unwrap();
    assert_eq!(
        book.member(new, Member::First),
        Ok(MemberStatus::Pending(pending_new))
    );
    let current = book.complete(
        pending_new,
        vec![ReturnedActor {
            actor: actor(1, 2),
            weapon: weapon(),
        }],
        &[],
    );
    assert_eq!(current.dispositions(), &[Disposition::Staged(actor(1, 2))]);
    book.acknowledge(current).unwrap();
}

#[test]
fn source_retirement_keeps_actor_obligations_until_explicit_confirmation() {
    let mut book = RequestBook::new(1, 1);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    stage(&mut book, source, Member::First, 1);
    book.invalidate(source).unwrap();
    assert_eq!(book.counts().retiring, 1);
    assert_eq!(book.forget(source), Err(SourceError::CleanupOutstanding));
    assert_eq!(
        book.confirm_retired(source, Member::First, actor(1, 2)),
        Err(SourceError::WrongRetirement)
    );
    book.confirm_retired(source, Member::First, actor(1, 1))
        .unwrap();
    book.forget(source).unwrap();
    assert_eq!(book.counts().sources, 0);
}

#[test]
fn failed_claim_restores_the_same_member_and_ammunition() {
    let mut book = RequestBook::new(1, 1);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    stage(&mut book, source, Member::First, 1);
    let result: Result<(), _> =
        book.claim(source, Member::First, Hand::Right, &table().snapshot(), Err);
    assert_eq!(result, Err(SourceError::ClaimRefused));
    assert_eq!(
        book.member(source, Member::First),
        Ok(MemberStatus::Staged(actor(1, 1)))
    );
    let claim = book
        .claim(source, Member::First, Hand::Right, &table().snapshot(), Ok)
        .unwrap();
    assert_eq!(claim.ammo.total(), 18);
}

#[test]
fn empty_wrong_type_and_verified_cancellation_allow_bounded_retry() {
    let mut book = RequestBook::new(1, 1);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let empty = book.begin(source, Member::First).unwrap();
    let completed = book.complete(empty, vec![], &[]);
    assert!(completed.dispositions().is_empty());
    book.acknowledge(completed).unwrap();
    let wrong = book.begin(source, Member::First).unwrap();
    let other_type = WeaponType::new(
        18,
        WeaponSlot::E_SLOT_DUAL_WIELD,
        WeaponClass::E_WEAPON_CLASS_PISTOL as i32,
    )
    .unwrap();
    let completed = book.complete(
        wrong,
        vec![ReturnedActor {
            actor: actor(1, 1),
            weapon: other_type,
        }],
        &[],
    );
    assert_eq!(
        completed.dispositions(),
        &[Disposition::Dispose(actor(1, 1))]
    );
    book.acknowledge(completed).unwrap();
    let cancelled = book.begin(source, Member::First).unwrap();
    book.cancel(cancelled).unwrap();
    book.confirm_cancelled(cancelled).unwrap();
    let unknown = book.complete(cancelled, vec![returned(2)], &[]);
    assert_eq!(unknown.dispositions(), &[Disposition::Dispose(actor(2, 1))]);
    book.acknowledge(unknown).unwrap();
    assert_eq!(book.counts().outstanding_requests, 0);
}

#[test]
fn cancelled_preparation_blocks_other_members_of_the_same_source() {
    let mut book = RequestBook::new(1, 2);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let request = book.begin(source, Member::First).unwrap();
    book.cancel(request).unwrap();
    assert_eq!(
        book.begin(source, Member::Second),
        Err(SourceError::SourceBusy)
    );
    book.confirm_cancelled(request).unwrap();
    assert!(book.begin(source, Member::Second).is_ok());
}

#[test]
fn duplicate_rejected_callback_waits_for_the_first_disposal_receipt() {
    let mut book = RequestBook::new(1, 1);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let request = book.begin(source, Member::First).unwrap();
    book.cancel(request).unwrap();
    let first = book.complete(request, vec![returned(1)], &[]);
    let duplicate = book.complete(request, vec![returned(1)], &[]);
    assert_eq!(
        duplicate.dispositions(),
        &[Disposition::CleanupPending(actor(1, 1))]
    );
    book.acknowledge(duplicate).unwrap();
    assert_eq!(book.counts().outstanding_requests, 1);
    book.acknowledge(first).unwrap();
    assert_eq!(book.counts().outstanding_requests, 0);
}

#[test]
fn recycled_address_callback_cannot_adopt_or_dispose_the_current_actor() {
    let mut book = RequestBook::new(1, 1);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let request = book.begin(source, Member::First).unwrap();
    let completion = book.complete(request, vec![returned(1)], &[actor(1, 2)]);
    assert_eq!(
        completion.dispositions(),
        &[Disposition::IdentityConflict(actor(1, 1))]
    );
    book.acknowledge(completion).unwrap();
    assert_eq!(
        book.member(source, Member::First),
        Ok(MemberStatus::Available)
    );
}

#[test]
fn second_cleanup_batch_blocks_other_requests_and_waits_for_both_acknowledgments() {
    for first_batch_first in [true, false] {
        let mut book = RequestBook::with_callback_limit(2, 2, 8);
        let first_source = key(1, 1);
        let second_source = key(2, 1);
        book.add_source(first_source, pair()).unwrap();
        book.add_source(second_source, pair()).unwrap();
        let first_request = book.begin(first_source, Member::First).unwrap();
        let second_request = book.begin(second_source, Member::First).unwrap();
        book.cancel(first_request).unwrap();
        let first = book.complete(first_request, vec![returned(1)], &[]);
        let second = book.complete(first_request, vec![returned(2)], &[]);
        assert_eq!(first.dispositions(), &[Disposition::Dispose(actor(1, 1))]);
        assert_eq!(second.dispositions(), &[Disposition::Dispose(actor(2, 1))]);
        assert_eq!(book.counts().pending_disposals, 2);
        let duplicate_second = book.complete(first_request, vec![returned(2)], &[]);
        assert_eq!(
            duplicate_second.dispositions(),
            &[Disposition::CleanupPending(actor(2, 1))]
        );
        let recycled_second = book.complete(
            first_request,
            vec![ReturnedActor {
                actor: actor(2, 2),
                weapon: weapon(),
            }],
            &[],
        );
        assert_eq!(
            recycled_second.dispositions(),
            &[Disposition::IdentityConflict(actor(2, 2))]
        );
        book.acknowledge(duplicate_second).unwrap();
        book.acknowledge(recycled_second).unwrap();
        let blocked_adoption = book.complete(second_request, vec![returned(2)], &[]);
        assert_eq!(
            blocked_adoption.dispositions(),
            &[Disposition::CleanupPending(actor(2, 1))]
        );
        assert_eq!(
            book.member(second_source, Member::First),
            Ok(MemberStatus::Pending(second_request))
        );
        book.acknowledge(blocked_adoption).unwrap();
        let (earlier, later, outstanding_actor) = if first_batch_first {
            (first, second, actor(2, 1))
        } else {
            (second, first, actor(1, 1))
        };
        book.acknowledge(earlier).unwrap();
        assert_eq!(book.counts().outstanding_requests, 1);
        assert_eq!(book.counts().pending_disposals, 1);
        assert_eq!(
            book.member(first_source, Member::First),
            Ok(MemberStatus::Pending(first_request))
        );
        assert_eq!(
            book.begin(first_source, Member::First),
            Err(SourceError::MemberUnavailable)
        );
        assert!(book.cleanup_pending(outstanding_actor));
        book.acknowledge(later).unwrap();
        assert_eq!(book.counts().outstanding_requests, 0);
        assert_eq!(book.counts().callback_batches, 0);
        assert_eq!(book.counts().pending_disposals, 0);
        assert_eq!(
            book.member(first_source, Member::First),
            Ok(MemberStatus::Available)
        );
        assert!(book.begin(first_source, Member::First).is_ok());
    }
}

#[test]
fn unknown_callback_disposal_is_retained_until_its_own_acknowledgment() {
    let mut book = RequestBook::new(1, 1);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let obsolete = book.activation_id().unwrap();
    let request = book.begin(source, Member::First).unwrap();
    let unknown = book.complete(obsolete, vec![returned(1)], &[]);
    assert_eq!(unknown.dispositions(), &[Disposition::Dispose(actor(1, 1))]);
    let current = book.complete(request, vec![returned(1)], &[]);
    assert_eq!(
        current.dispositions(),
        &[Disposition::CleanupPending(actor(1, 1))]
    );
    book.acknowledge(current).unwrap();
    assert_eq!(book.counts().outstanding_requests, 0);
    assert_eq!(book.counts().pending_disposals, 1);
    book.acknowledge(unknown).unwrap();
    assert_eq!(book.counts().pending_disposals, 0);
}

#[test]
fn callback_batch_saturation_returns_all_identities_and_keeps_requests_blocked() {
    let mut book = RequestBook::with_callback_limit(2, 2, 2);
    let first_source = key(1, 1);
    let second_source = key(2, 1);
    book.add_source(first_source, pair()).unwrap();
    book.add_source(second_source, pair()).unwrap();
    let first_request = book.begin(first_source, Member::First).unwrap();
    let second_request = book.begin(second_source, Member::First).unwrap();
    book.cancel(first_request).unwrap();
    let first = book.complete(first_request, vec![returned(1)], &[]);
    let duplicate = book.complete(first_request, vec![returned(1)], &[]);
    assert_eq!(book.counts().callback_batches, 2);
    assert_eq!(book.counts().pending_disposals, 1);
    let rejected = book.complete(first_request, vec![returned(2), returned(2)], &[]);
    let rejected = book.acknowledge(rejected).unwrap_err();
    let Completion::Rejected(failure) = rejected else {
        panic!("capacity returns the whole batch");
    };
    assert_eq!(failure.reason, CompletionFailureReason::Capacity);
    assert_eq!(failure.request, first_request);
    assert_eq!(failure.returned, vec![returned(2), returned(2)]);
    assert!(book.counts().completion_blocked);
    assert_eq!(book.counts().callback_batches, 2);
    assert_eq!(book.counts().pending_disposals, 1);
    let blocked = book.complete(second_request, vec![returned(2)], &[]);
    let Completion::Rejected(blocked) = blocked else {
        panic!("a saturated book cannot adopt");
    };
    assert_eq!(blocked.reason, CompletionFailureReason::Blocked);
    assert_eq!(blocked.returned, vec![returned(2)]);
    assert_eq!(
        book.member(second_source, Member::First),
        Ok(MemberStatus::Pending(second_request))
    );
    book.acknowledge(first).unwrap();
    book.acknowledge(duplicate).unwrap();
    assert_eq!(book.counts().callback_batches, 0);
    assert_eq!(book.counts().outstanding_requests, 2);
    assert_eq!(
        book.member(first_source, Member::First),
        Ok(MemberStatus::Pending(first_request))
    );
    assert_eq!(
        book.begin(first_source, Member::Second),
        Err(SourceError::CompletionBlocked)
    );
    assert_eq!(book.activation_id(), Err(SourceError::CompletionBlocked));
    assert_eq!(
        book.claim(
            first_source,
            Member::First,
            Hand::Right,
            &table().snapshot(),
            Ok
        ),
        Err(SourceError::CompletionBlocked)
    );
}

#[test]
fn disposal_identity_saturation_rejects_the_whole_batch_without_partial_adoption() {
    let mut book = RequestBook::with_callback_limit(1, 1, 2);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let request = book.begin(source, Member::First).unwrap();
    let returned = vec![returned(1), returned(2), returned(3)];
    let Completion::Rejected(failure) = book.complete(request, returned.clone(), &[]) else {
        panic!("a batch over the identity limit is returned intact");
    };
    assert_eq!(failure.reason, CompletionFailureReason::Capacity);
    assert_eq!(failure.returned, returned);
    assert_eq!(book.counts().callback_batches, 0);
    assert_eq!(book.counts().pending_disposals, 0);
    assert_eq!(book.counts().outstanding_requests, 1);
    assert_eq!(
        book.member(source, Member::First),
        Ok(MemberStatus::Pending(request))
    );
    book.cancel(request).unwrap();
    assert_eq!(
        book.confirm_cancelled(request),
        Err(SourceError::CompletionBlocked)
    );
}

#[test]
fn cumulative_disposal_saturation_preserves_prior_receipts_and_the_rejected_batch() {
    let mut book = RequestBook::with_callback_limit(1, 1, 2);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let request = book.begin(source, Member::First).unwrap();
    let first = book.complete(request, vec![returned(1), returned(2)], &[]);
    assert_eq!(book.counts().callback_batches, 1);
    assert_eq!(book.counts().pending_disposals, 2);
    let Completion::Rejected(failure) = book.complete(request, vec![returned(3)], &[]) else {
        panic!("retained disposal identities have a bound independent of batch count");
    };
    assert_eq!(failure.reason, CompletionFailureReason::Capacity);
    assert_eq!(failure.returned, vec![returned(3)]);
    assert_eq!(book.counts().callback_batches, 1);
    assert_eq!(book.counts().pending_disposals, 2);
    assert!(book.cleanup_pending(actor(1, 1)));
    assert!(book.cleanup_pending(actor(2, 1)));
    book.acknowledge(first).unwrap();
    assert_eq!(book.counts().outstanding_requests, 1);
    assert_eq!(book.counts().pending_disposals, 0);
    assert!(book.counts().completion_blocked);
}

#[test]
fn zero_callback_capacity_returns_an_adoptable_actor_without_staging_it() {
    let mut book = RequestBook::with_callback_limit(1, 1, 0);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let request = book.begin(source, Member::First).unwrap();
    let Completion::Rejected(failure) = book.complete(request, vec![returned(1)], &[]) else {
        panic!("no receipt capacity means no staging commit");
    };
    assert_eq!(failure.reason, CompletionFailureReason::Capacity);
    assert_eq!(failure.returned, vec![returned(1)]);
    assert_eq!(
        book.member(source, Member::First),
        Ok(MemberStatus::Pending(request))
    );
    assert_eq!(book.counts().staged, 0);
}

#[test]
fn settlement_identity_exhaustion_returns_the_batch_instead_of_reusing_a_receipt() {
    let mut book = RequestBook::with_callback_limit(1, 1, 4);
    let source = key(1, 1);
    book.add_source(source, pair()).unwrap();
    let request = book.begin(source, Member::First).unwrap();
    book.cancel(request).unwrap();
    book.next_settlement = NonZeroU64::new(u64::MAX);
    let last = book.complete(request, vec![returned(1)], &[]);
    let Completion::Rejected(failure) = book.complete(request, vec![returned(2)], &[]) else {
        panic!("receipt identities cannot wrap");
    };
    assert_eq!(failure.reason, CompletionFailureReason::IdentityExhausted);
    assert_eq!(failure.returned, vec![returned(2)]);
    book.acknowledge(last).unwrap();
    assert_eq!(book.counts().outstanding_requests, 1);
    assert!(book.counts().completion_blocked);
}

#[test]
fn source_and_request_identity_limits_do_not_wrap_or_grow() {
    let mut book = RequestBook::new(1, 1);
    book.add_source(key(1, 1), pair()).unwrap();
    assert_eq!(
        book.add_source(key(2, 1), pair()),
        Err(SourceError::Capacity)
    );
    book.ids = RequestIds {
        next: NonZeroU64::new(u64::MAX),
    };
    let last = book.activation_id().unwrap();
    assert_eq!(book.activation_id(), Err(SourceError::IdentityExhausted));
    assert_eq!(last, RequestId(NonZeroU64::new(u64::MAX).unwrap()));
}
