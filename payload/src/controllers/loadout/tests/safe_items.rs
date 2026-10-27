//! Production safe-item moves alongside the actual native loadout transitions.

use std::num::NonZeroU64;

use jc3gi::character::inventory::WeaponSlot;
use pretty_assertions::assert_eq;

use crate::controllers::{
    frame::Hand,
    loadout::{
        BodyLayout, Event, ResetKind, Socket,
        binding::Binding,
        item::ItemKind,
        projection::{SlotContent, project_store},
        safe_items::{ItemLeaseId, SafeItemError},
        table::{Departure, Loadout, Place},
        tests::fixtures::{a_pistol, check_invariants, object, take, weapon},
        transition::{Release, Removal, RemovalWhy, TransitionError},
    },
};

#[test]
fn independent_safe_items_resolve_through_draw_transfer_and_stow() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let second = table.admit_safe_item(model(2), Hand::Right).unwrap();
    assert_ne!(first.item, second.item);
    assert_eq!(table.primary(), None);
    assert_eq!(table.snapshot().items.len(), 2);
    check_invariants(&table);
    check_safe_items(&table);

    let stowed = table
        .move_safe_item(first, Place::Socket(Socket::Hip(Hand::Left)))
        .unwrap();
    assert_eq!(table.safe_item(first.item), Some(stowed));
    assert_eq!(table.safe_item_for_lease(model(2)), Some(second));
    check_safe_items(&table);
    let drawn = table
        .move_safe_item(stowed, Place::Hand(Hand::Left))
        .unwrap();
    let second = table
        .move_safe_item(second, Place::Socket(Socket::Chest))
        .unwrap();
    let transferred = table
        .move_safe_item(drawn, Place::Hand(Hand::Right))
        .unwrap();
    assert_eq!(transferred.item, first.item);
    assert_eq!(transferred.binding.lease, first.binding.lease);
    assert_eq!(transferred.binding.revision(), 3);
    assert_eq!(table.safe_item_for_lease(model(1)), Some(transferred));
    assert_eq!(table.safe_item(second.item), Some(second));
    assert_eq!(table.held(Hand::Left), None);
    check_safe_items(&table);
}

#[test]
fn duplicate_lease_occupied_hand_and_invalid_holster_leave_the_table_unchanged() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let second = table.admit_safe_item(model(2), Hand::Right).unwrap();
    let before = table.snapshot();
    let next_id = table.next_id;
    assert_eq!(
        table.admit_safe_item(model(1), Hand::Right),
        Err(SafeItemError::LeaseAlreadyBound {
            model: model(1),
            item: first.item,
        })
    );
    assert_eq!(
        table.admit_safe_item(model(3), Hand::Right),
        Err(SafeItemError::Occupied {
            place: second.place,
            item: second.item,
        })
    );
    assert_eq!(
        table.move_safe_item(first, second.place),
        Err(SafeItemError::Occupied {
            place: second.place,
            item: second.item,
        })
    );
    assert_eq!(
        table.move_safe_item(first, Place::Socket(Socket::Shoulder(Hand::Left))),
        Err(SafeItemError::UnsupportedHolster(Socket::Shoulder(
            Hand::Left
        )))
    );
    assert_eq!(table.snapshot(), before);
    assert_eq!(table.next_id, next_id);
    check_safe_items(&table);
}

#[test]
fn safe_items_share_native_occupancy_without_rebinding_or_replacing_a_weapon() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let native = take(&mut table, Hand::Right, 1, weapon(a_pistol()));
    assert_eq!(table.safe_item(native), None);
    let before = table.snapshot();
    assert_eq!(
        table.admit_safe_item(model(1), Hand::Right),
        Err(SafeItemError::Occupied {
            place: Place::Hand(Hand::Right),
            item: native,
        })
    );
    assert_eq!(table.snapshot(), before);
    let safe = table.admit_safe_item(model(1), Hand::Left).unwrap();
    assert_eq!(table.primary(), Some(Hand::Right));
    check_invariants(&table);
    table.dock(Hand::Right, Socket::Chest).unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.move_safe_item(safe, Place::Socket(Socket::Chest)),
        Err(SafeItemError::Occupied {
            place: Place::Socket(Socket::Chest),
            item: native,
        })
    );
    assert_eq!(table.snapshot(), before);
    assert_eq!(
        table
            .items()
            .find(|item| item.id == native)
            .unwrap()
            .binding,
        Binding::Live(object(1))
    );
    assert_eq!(table.safe_item_for_lease(model(1)), Some(safe));
    check_invariants(&table);
    let stowed = table
        .move_safe_item(safe, Place::Socket(Socket::Hip(Hand::Left)))
        .unwrap();
    table.draw(Socket::Chest, Hand::Right).unwrap();
    assert_eq!(
        table.held(Hand::Right).unwrap().binding,
        Binding::Live(object(1))
    );
    assert_eq!(table.safe_item(safe.item), Some(stowed));
    check_safe_items(&table);
}

#[test]
fn stale_views_cannot_move_or_retire_an_item_after_it_returns_to_the_same_hand() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let original = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let moved = table
        .move_safe_item(original, Place::Hand(Hand::Right))
        .unwrap();
    let current = table.move_safe_item(moved, original.place).unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.move_safe_item(original, Place::Socket(Socket::Chest)),
        Err(SafeItemError::StaleItem(original.item))
    );
    assert_eq!(
        table.retire_safe_item(original),
        Err(SafeItemError::StaleItem(original.item))
    );
    assert_eq!(table.snapshot(), before);
    assert_eq!(table.safe_item(original.item), Some(current));
    check_safe_items(&table);
}

#[test]
fn inert_drops_keep_identity_and_retirement_invalidates_only_that_identity() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let second = table.admit_safe_item(model(2), Hand::Right).unwrap();
    let before = table.snapshot();
    for departure in [Departure::AtSpeed, Departure::Returned] {
        assert_eq!(
            table.move_safe_item(first, Place::World(departure)),
            Err(SafeItemError::UnsupportedDeparture(departure))
        );
        assert_eq!(table.snapshot(), before);
    }
    let dropped = table
        .move_safe_item(first, Place::World(Departure::Dropped))
        .unwrap();
    assert_eq!(dropped.item, first.item);
    assert_eq!(dropped.binding.lease, first.binding.lease);
    assert_eq!(table.safe_item(first.item), Some(dropped));
    assert_eq!(table.safe_item_for_lease(model(1)), Some(dropped));
    assert_eq!(table.world.len(), 0);
    assert_eq!(table.safe_world.len(), 1);
    assert_eq!(table.held(Hand::Left), None);
    check_safe_items(&table);
    assert_eq!(table.retire_safe_item(dropped), Ok(model(1)));
    assert_eq!(table.safe_item(first.item), None);
    assert_eq!(table.safe_item_for_lease(model(1)), None);
    assert_eq!(table.safe_item(second.item), Some(second));
    assert_eq!(
        table.retire_safe_item(dropped),
        Err(SafeItemError::UnknownItem(first.item))
    );
    assert_eq!(
        table.move_safe_item(first, Place::Hand(Hand::Left)),
        Err(SafeItemError::UnknownItem(first.item))
    );
    let replacement = table.admit_safe_item(model(3), Hand::Left).unwrap();
    assert_ne!(replacement.item, first.item);
    let second = table
        .move_safe_item(second, Place::Socket(Socket::Chest))
        .unwrap();
    assert_eq!(table.retire_safe_item(second), Ok(model(2)));
    assert_eq!(table.safe_item_for_lease(model(2)), None);
    assert_eq!(table.safe_item(replacement.item), Some(replacement));
    check_safe_items(&table);
}

#[test]
fn independent_world_models_share_no_occupancy_or_native_address() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let second = table.admit_safe_item(model(2), Hand::Right).unwrap();
    let first = table
        .move_safe_item(first, Place::World(Departure::Dropped))
        .unwrap();
    let second = table
        .move_safe_item(second, Place::World(Departure::Dropped))
        .unwrap();
    assert_eq!(first.place, second.place);
    assert_eq!(table.safe_items().count(), 2);
    let recovered = table
        .move_safe_item(first, Place::Hand(Hand::Right))
        .unwrap();
    assert_eq!(recovered.item, first.item);
    assert_eq!(recovered.binding.lease, first.binding.lease);
    assert_eq!(recovered.binding.revision(), 2);
    let before = table.snapshot();
    assert_eq!(
        table.move_safe_item(second, Place::Hand(Hand::Right)),
        Err(SafeItemError::Occupied {
            place: recovered.place,
            item: recovered.item,
        })
    );
    assert_eq!(
        table.retire_safe_item(first),
        Err(SafeItemError::StaleItem(first.item))
    );
    assert_eq!(table.snapshot(), before);
    let native = take(&mut table, Hand::Left, 1, weapon(a_pistol()));
    assert_eq!(table.primary(), Some(Hand::Left));
    let before = table.snapshot();
    assert_eq!(
        table.move_safe_item(second, Place::Hand(Hand::Left)),
        Err(SafeItemError::Occupied {
            place: Place::Hand(Hand::Left),
            item: native,
        })
    );
    assert_eq!(table.snapshot(), before);
    assert_eq!(table.safe_item(second.item), Some(second));
    assert_eq!(table.world.len(), 0);
    check_safe_items(&table);
}

#[test]
fn missing_holster_and_exhausted_revision_do_not_move_the_item() {
    let mut table = Loadout::new(BodyLayout::new(Hand::Right, 0, 0, false));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.move_safe_item(first, Place::Socket(Socket::Chest)),
        Err(SafeItemError::UnsupportedHolster(Socket::Chest))
    );
    assert_eq!(table.snapshot(), before);
    let Some(item) = table.hands[Hand::Left.index()].as_mut() else {
        panic!("held item")
    };
    let Binding::SafeItem(binding) = &mut item.binding else {
        panic!("safe binding")
    };
    binding.exhaust_revision();
    let current = table.safe_item(first.item).unwrap();
    let before = table.snapshot();
    for destination in [Place::Hand(Hand::Right), Place::World(Departure::Dropped)] {
        assert_eq!(
            table.move_safe_item(current, destination),
            Err(SafeItemError::RevisionExhausted)
        );
        assert_eq!(table.snapshot(), before);
    }
    assert_eq!(table.move_safe_item(current, current.place), Ok(current));
    check_safe_items(&table);
}

#[test]
fn a_holstered_item_must_be_drawn_before_restowing_and_can_leave_a_removed_holster() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let stowed = table
        .move_safe_item(first, Place::Socket(Socket::Hip(Hand::Left)))
        .unwrap();
    let before = table.snapshot();
    for destination in [
        Place::Socket(Socket::Chest),
        Place::World(Departure::Dropped),
    ] {
        assert_eq!(
            table.move_safe_item(stowed, destination),
            Err(SafeItemError::NotHeld(first.item))
        );
        assert_eq!(table.snapshot(), before);
    }
    table.set_body(BodyLayout::new(Hand::Right, 0, 0, false));
    let drawn = table
        .move_safe_item(stowed, Place::Hand(Hand::Right))
        .unwrap();
    assert_eq!(drawn.item, first.item);
    assert_eq!(drawn.binding.lease, first.binding.lease);
    check_safe_items(&table);
}

#[test]
fn native_table_events_cannot_bypass_checked_safe_moves_or_retirement() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let safe = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let before = table.snapshot();
    for event in [
        Event::Dock {
            hand: Hand::Left,
            socket: Socket::Chest,
        },
        Event::HandOver { from: Hand::Left },
        Event::LetGo {
            hand: Hand::Left,
            release: Release::Dropped,
        },
        Event::Removed {
            target: Removal::Item(safe.item),
            why: RemovalWhy::Gone,
        },
    ] {
        assert_eq!(
            table.fold(event),
            Err(TransitionError::SafeItemRequiresCheckedMove(safe.item))
        );
        assert_eq!(table.snapshot(), before);
    }
    let stowed = table
        .move_safe_item(safe, Place::Socket(Socket::Chest))
        .unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.draw(Socket::Chest, Hand::Right),
        Err(TransitionError::SafeItemRequiresCheckedMove(safe.item))
    );
    assert_eq!(table.snapshot(), before);
    let drawn = table
        .move_safe_item(stowed, Place::Hand(Hand::Right))
        .unwrap();
    let loose = table
        .move_safe_item(drawn, Place::World(Departure::Dropped))
        .unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.remove(Removal::Item(loose.item), RemovalWhy::Gone),
        Err(TransitionError::SafeItemRequiresCheckedMove(loose.item))
    );
    assert_eq!(table.snapshot(), before);
    assert_eq!(table.retire_safe_item(loose), Ok(model(1)));
}

#[test]
fn native_weapon_events_respect_a_safe_items_hand_and_holster() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let safe = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let native = take(&mut table, Hand::Right, 1, weapon(a_pistol()));
    let before = table.snapshot();
    assert_eq!(
        table.hand_over(Hand::Right),
        Err(TransitionError::HandOccupied {
            hand: Hand::Left,
            item: safe.item
        })
    );
    assert_eq!(table.snapshot(), before);
    let stowed = table
        .move_safe_item(safe, Place::Socket(Socket::Chest))
        .unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.dock(Hand::Right, Socket::Chest),
        Err(TransitionError::SocketOccupied {
            socket: Socket::Chest,
            item: safe.item
        })
    );
    assert_eq!(table.snapshot(), before);
    assert_eq!(table.held(Hand::Right).unwrap().id, native);
    assert_eq!(table.safe_item(safe.item), Some(stowed));
    check_safe_items(&table);
}

#[test]
fn resets_remove_safe_model_identities_from_every_container() {
    for kind in ResetKind::ALL {
        let mut table = Loadout::new(BodyLayout::full(Hand::Right));
        let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
        let stowed = table
            .move_safe_item(first, Place::Socket(Socket::Chest))
            .unwrap();
        let second = table.admit_safe_item(model(2), Hand::Left).unwrap();
        let loose = table
            .move_safe_item(second, Place::World(Departure::Dropped))
            .unwrap();
        let held = table.admit_safe_item(model(3), Hand::Right).unwrap();
        assert!(table.begin_reset().is_empty());
        assert_eq!(table.safe_items().count(), 3);
        let removed = table.complete_reset(kind, &[]).unwrap();
        assert_eq!(removed.len(), 3);
        assert_eq!(table.safe_items().count(), 0);
        assert!(table.safe_world.is_empty());
        for item in [stowed, loose, held] {
            assert_eq!(table.safe_item(item.item), None);
            assert_eq!(
                table.retire_safe_item(item),
                Err(SafeItemError::UnknownItem(item.item))
            );
        }
        let next = table.admit_safe_item(model(4), Hand::Right).unwrap();
        assert!(next.item > held.item);
        check_safe_items(&table);
    }
}

#[test]
fn safe_items_never_project_into_native_slots_or_weapon_caches() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let safe = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let native = take(&mut table, Hand::Right, 1, weapon(a_pistol()));
    let projection = table.snapshot().projection;
    let store = project_store(&table);
    assert_eq!(projection.slot_of(safe.item), None);
    assert_eq!(
        projection.slot_of(native),
        Some(WeaponSlot::E_SLOT_DUAL_WIELD)
    );
    assert_eq!(store.sidecar.len(), 1);
    assert_eq!(store.sidecar[0].item, native);
    assert!(projection.slots.iter().any(|slot| {
        slot.slot == WeaponSlot::E_SLOT_HAND_GRENADE && slot.content == SlotContent::Game
    }));
    check_safe_items(&table);
}

fn model(n: u64) -> ItemLeaseId {
    ItemLeaseId::new(NonZeroU64::new(n).unwrap())
}

fn check_safe_items(table: &Loadout) {
    check_invariants(table);
    let items: Vec<_> = table.items().collect();
    for (index, item) in items.iter().enumerate() {
        assert!(items[..index].iter().all(|other| other.id != item.id));
        if !matches!(item.place, Place::World(_)) {
            assert!(items[..index].iter().all(|other| other.place != item.place));
        }
        let Some(safe) = table.safe_item(item.id) else {
            continue;
        };
        assert_eq!(item.kind, ItemKind::Grenade);
        assert_eq!(item.binding.address(), None);
        assert_eq!(item.binding.safe_item(), Some(safe.binding));
        assert_eq!(safe.place, item.place);
        assert_eq!(table.safe_item_for_lease(safe.binding.lease), Some(safe));
        assert!(items[..index].iter().all(|other| {
            other
                .binding
                .safe_item()
                .is_none_or(|binding| binding.lease != safe.binding.lease)
        }));
        assert_eq!(table.snapshot().projection.slot_of(item.id), None);
        match safe.place {
            Place::Hand(_)
            | Place::Socket(Socket::Hip(_) | Socket::Chest)
            | Place::World(Departure::Dropped) => {}
            _ => panic!("safe item outside a hand, a holster, or an inert drop"),
        }
    }
}
