use std::{cell::Cell, num::NonZeroUsize};

use crate::controllers::weapons::reload::{
    engine::{DetachOutcome, finish_detach},
    loose::Loose,
};

#[test]
fn no_throw_never_spawns_or_commits_any_weapon_change() {
    let spawned = Cell::new(false);
    let committed = Cell::new(false);
    let before = (17, 70, false, true, true);
    let weapon = Cell::new(before);
    let result = finish_detach(
        (false, None),
        || spawned.set(true),
        |_| {
            committed.set(true);
            weapon.set((0, 70, true, true, false));
        },
    );
    assert_eq!(result, DetachOutcome::NoThrow);
    assert_eq!(weapon.get(), before);
    assert!(!spawned.get());
    assert!(!committed.get());
}

#[test]
fn unretained_drop_does_not_clear_ammo_or_refund_reserve() {
    let spawned = Cell::new(0);
    let weapon = Cell::new((17, 70, false));
    let result = finish_detach(
        (true, None),
        || spawned.set(spawned.get() + 1),
        |retained| {
            let (ammo, reserve, _) = weapon.get();
            weapon.set((if retained { 0 } else { ammo }, reserve, true));
        },
    );
    assert_eq!(result, DetachOutcome::DroppedUnretained);
    assert_eq!(weapon.get(), (17, 70, true));
    assert_eq!(spawned.get(), 1);
}

#[test]
fn retained_drop_commits_exactly_one_ammo_transfer_after_owner_read_callback() {
    let magazine = Loose::test(
        NonZeroUsize::new(1).unwrap(),
        1,
        NonZeroUsize::new(1).unwrap(),
        1,
        17,
    );
    let weapon = Cell::new((17, 70, false));
    let result = finish_detach(
        (true, Some(magazine)),
        || {
            // The fake spawn re-enters the owner before the detached ammo commit.
            assert_eq!(weapon.get(), (17, 70, false));
        },
        |retained| {
            assert!(retained);
            let (_, reserve, _) = weapon.get();
            weapon.set((0, reserve, true));
        },
    );
    assert_eq!(result, DetachOutcome::Retained(magazine));
    assert_eq!(weapon.get(), (0, 70, true));
}
