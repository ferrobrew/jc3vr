//! Synthetic grenade plans. No fixture creates or claims a live explosive actor.

use super::*;
use crate::controllers::loadout::{
    BodyLayout, ItemKind, ObjectAddress,
    lifecycle_source::RequestIds,
    table::Loadout,
    transition::{Take, TakeSource},
};

fn table() -> Loadout {
    Loadout::new(BodyLayout::full(Hand::Right))
}

fn held(table: &mut Loadout, hand: Hand, address: usize) -> GrenadeView {
    let item = table
        .take(Take {
            hand,
            object: ObjectAddress::from_address(address).unwrap(),
            kind: ItemKind::Grenade,
            source: TakeSource::Pouch,
        })
        .unwrap()[0]
        .item;
    GrenadeView {
        item,
        revision: 0,
        place: Place::Hand(hand),
        state: GrenadeState::Safe,
    }
}

fn actor(address: usize) -> ActorKey {
    ActorKey {
        address: ObjectAddress::from_address(address).unwrap(),
        generation: Generation(1),
    }
}

#[test]
fn safe_transfer_holster_and_drop_keep_one_identity_and_no_actor() {
    let mut table = table();
    let held = held(&mut table, Hand::Left, 1);
    let occupancy = table.snapshot();
    let GrenadePlan::Commit(transfer) =
        plan(held, GrenadeAction::MoveToHand(Hand::Right), &occupancy).unwrap()
    else {
        panic!("a transfer does not activate");
    };
    assert_eq!(transfer.next.item, held.item);
    assert_eq!(transfer.next.place, Place::Hand(Hand::Right));
    let GrenadePlan::Commit(dock) = plan(
        held,
        GrenadeAction::Dock(Socket::Hip(Hand::Left)),
        &occupancy,
    )
    .unwrap() else {
        panic!("a safe dock does not activate");
    };
    assert_eq!(dock.next.place, Place::Socket(Socket::Hip(Hand::Left)));
    assert_eq!(dock.next.state, GrenadeState::Safe);
    let GrenadePlan::Commit(drop) = plan(held, GrenadeAction::IncidentalDrop, &occupancy).unwrap()
    else {
        panic!("an incidental safe drop stays safe");
    };
    assert_eq!(drop.next.item, held.item);
    assert_eq!(drop.next.state, GrenadeState::Safe);
    assert_eq!(drop.next.place, Place::World(Departure::Dropped));
}

#[test]
fn occupied_hands_and_holsters_refuse_without_releasing_the_item() {
    let mut table = table();
    let first = held(&mut table, Hand::Left, 1);
    held(&mut table, Hand::Right, 2);
    let snapshot = table.snapshot();
    assert_eq!(
        plan(first, GrenadeAction::MoveToHand(Hand::Right), &snapshot),
        Err(GrenadeError::Occupied(Place::Hand(Hand::Right)))
    );
    let mut occupied = snapshot.clone();
    occupied.items[1].place = Place::Socket(Socket::Chest);
    assert_eq!(
        plan(first, GrenadeAction::Dock(Socket::Chest), &occupied),
        Err(GrenadeError::Occupied(Place::Socket(Socket::Chest)))
    );
    assert_eq!(table.snapshot(), snapshot);
    assert_eq!(
        plan(first, GrenadeAction::Dock(Socket::BackOuter), &snapshot),
        Err(GrenadeError::UnsupportedHolster(Socket::BackOuter))
    );
}

#[test]
fn deliberate_throw_autoarms_only_after_success_and_completes_once() {
    let mut table = table();
    let item = held(&mut table, Hand::Left, 1);
    let snapshot = table.snapshot();
    let request = RequestIds::default().issue().unwrap();
    let planned = plan(item, GrenadeAction::DeliberateThrow, &snapshot).unwrap();
    let mut pending = Activation::begin(planned, request, Generation(1)).unwrap();
    assert!(pending.outstanding());
    assert_eq!(item.state, GrenadeState::Safe);
    let commit = complete_activation(
        &mut pending,
        request,
        Generation(1),
        item,
        actor(9),
        &snapshot,
    )
    .unwrap();
    assert_eq!(commit.next.item, item.item);
    assert_eq!(commit.next.place, Place::World(Departure::Dropped));
    assert_eq!(commit.next.state, GrenadeState::Active(actor(9)));
    assert!(!pending.outstanding());
    assert_eq!(
        complete_activation(
            &mut pending,
            request,
            Generation(1),
            item,
            actor(9),
            &snapshot
        ),
        Err(GrenadeError::AlreadyAdopted(actor(9)))
    );
    assert_eq!(
        complete_activation(
            &mut pending,
            request,
            Generation(1),
            item,
            actor(10),
            &snapshot
        ),
        Err(GrenadeError::StaleActivation)
    );
}

#[test]
fn cancelled_failed_moved_and_old_world_activations_leave_safe_items_intact() {
    let mut table = table();
    let item = held(&mut table, Hand::Left, 1);
    let snapshot = table.snapshot();
    let mut ids = RequestIds::default();
    for scenario in 0..4 {
        let request = ids.issue().unwrap();
        let mut pending = Activation::begin(
            plan(item, GrenadeAction::Arm, &snapshot).unwrap(),
            request,
            Generation(1),
        )
        .unwrap();
        match scenario {
            0 => pending.cancel(),
            1 => {
                pending.allocation_failed(request).unwrap();
                assert!(!pending.outstanding());
            }
            _ => {}
        }
        let current = if scenario == 2 {
            GrenadeView {
                revision: 1,
                ..item
            }
        } else {
            item
        };
        let world = if scenario == 3 {
            Generation(2)
        } else {
            Generation(1)
        };
        assert_eq!(
            complete_activation(&mut pending, request, world, current, actor(9), &snapshot),
            Err(GrenadeError::StaleActivation)
        );
        assert_eq!(item.state, GrenadeState::Safe);
        assert!(!pending.outstanding());
    }
}

#[test]
fn unrelated_activation_callback_does_not_consume_the_pending_request() {
    let mut table = table();
    let item = held(&mut table, Hand::Left, 1);
    let snapshot = table.snapshot();
    let mut ids = RequestIds::default();
    let request = ids.issue().unwrap();
    let unrelated = ids.issue().unwrap();
    let mut pending = Activation::begin(
        plan(item, GrenadeAction::Arm, &snapshot).unwrap(),
        request,
        Generation(1),
    )
    .unwrap();
    assert_eq!(
        complete_activation(
            &mut pending,
            unrelated,
            Generation(1),
            item,
            actor(9),
            &snapshot
        ),
        Err(GrenadeError::StaleActivation)
    );
    assert!(pending.outstanding());
    assert!(
        complete_activation(
            &mut pending,
            request,
            Generation(1),
            item,
            actor(9),
            &snapshot
        )
        .is_ok()
    );
}

#[test]
fn active_transfer_and_release_preserve_actor_and_cannot_dock_or_rearm() {
    let mut table = table();
    let safe = held(&mut table, Hand::Left, 1);
    let active = GrenadeView {
        state: GrenadeState::Active(actor(9)),
        ..safe
    };
    let snapshot = table.snapshot();
    for action in [
        GrenadeAction::MoveToHand(Hand::Right),
        GrenadeAction::IncidentalDrop,
        GrenadeAction::DeliberateThrow,
    ] {
        let GrenadePlan::Commit(commit) = plan(active, action, &snapshot).unwrap() else {
            panic!("an existing actor is not activated again");
        };
        assert_eq!(commit.next.state, active.state);
        assert_eq!(commit.next.item, active.item);
    }
    assert_eq!(
        plan(active, GrenadeAction::Arm, &snapshot),
        Err(GrenadeError::AlreadyActive)
    );
    assert_eq!(
        plan(active, GrenadeAction::Dock(Socket::Chest), &snapshot),
        Err(GrenadeError::ActiveCannotDock)
    );
}

#[test]
fn independent_safe_items_redraw_and_regrip_without_changing_identity() {
    let mut table = table();
    let first = held(&mut table, Hand::Left, 1);
    let second = held(&mut table, Hand::Right, 2);
    assert_ne!(first.item, second.item);
    let mut snapshot = table.snapshot();
    snapshot.items[0].place = Place::Socket(Socket::Chest);
    let docked = GrenadeView {
        place: Place::Socket(Socket::Chest),
        revision: 1,
        ..first
    };
    let GrenadePlan::Commit(redraw) =
        plan(docked, GrenadeAction::MoveToHand(Hand::Left), &snapshot).unwrap()
    else {
        panic!("redraw remains safe");
    };
    assert_eq!(redraw.next.item, first.item);
    assert_eq!(redraw.next.state, GrenadeState::Safe);
    snapshot.items[0].place = redraw.next.place;
    let GrenadePlan::Commit(drop) =
        plan(redraw.next, GrenadeAction::IncidentalDrop, &snapshot).unwrap()
    else {
        panic!("drop remains safe");
    };
    snapshot.items[0].place = drop.next.place;
    let GrenadePlan::Commit(regrip) =
        plan(drop.next, GrenadeAction::MoveToHand(Hand::Left), &snapshot).unwrap()
    else {
        panic!("regrip remains safe");
    };
    assert_eq!(regrip.next.item, first.item);
    assert_eq!(snapshot.items[1].id, second.item);
    assert_eq!(second.state, GrenadeState::Safe);
}

#[test]
fn actor_removal_uses_the_existing_item_removal_and_rejects_recycled_identity() {
    let mut table = table();
    let safe = held(&mut table, Hand::Left, 1);
    assert_eq!(actor_removed(safe, actor(9)), None);
    let active = GrenadeView {
        state: GrenadeState::Active(actor(9)),
        ..safe
    };
    assert_eq!(
        actor_removed(active, actor(9)),
        Some(Removal::Item(safe.item))
    );
    let recycled = ActorKey {
        generation: Generation(2),
        ..actor(9)
    };
    assert_eq!(actor_removed(active, recycled), None);
}

#[test]
fn revision_exhaustion_refuses_instead_of_reusing_an_activation_generation() {
    let mut table = table();
    let item = GrenadeView {
        revision: u64::MAX,
        ..held(&mut table, Hand::Left, 1)
    };
    assert_eq!(
        plan(item, GrenadeAction::IncidentalDrop, &table.snapshot()),
        Err(GrenadeError::RevisionExhausted)
    );
}
