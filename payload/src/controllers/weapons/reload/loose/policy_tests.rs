use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

use super::*;
use crate::controllers::weapons::reload::{
    engine::{DetachOutcome, finish_detach},
    loose::records::CAPACITY,
};

fn key(index: usize) -> DropKey {
    DropKey {
        gun: index + 1,
        spare: index + 100,
    }
}

fn captured(index: usize, rounds: i32) -> Captured {
    Captured {
        object: NonZeroUsize::new(key(index).spare).unwrap(),
        body: index as u32,
        owner: NonZeroUsize::new(index + 1000).unwrap(),
        tag: 1,
        rounds,
    }
}

#[test]
fn automatic_local_and_empty_npc_admission_is_bounded_and_full_runs_native_unchanged() {
    for local in [false, true] {
        let records = RefCell::new(Records::new());
        let calls = Cell::new(0);
        let native = Cell::new(if local {
            (17, 70, false)
        } else {
            (0, 70, false)
        });
        for index in 0..CAPACITY * 4 {
            let rounds = automatic_rounds(true, local, Some(native.get().0)).unwrap();
            let token = records.borrow_mut().reserve(key(index));
            let expected = token.map(|token| ExpectedDrop {
                key: key(index),
                token,
                rounds,
            });
            let before = native.get();
            let result = dispatch_drop(
                expected,
                true,
                || Some((key(index), 1)),
                |e, k| records.borrow_mut().begin(e.token, k),
                || {
                    calls.set(calls.get() + 1);
                    true
                },
                |e, _| {
                    records
                        .borrow_mut()
                        .commit(e.token, captured(index, e.rounds), Instant::now())
                },
            );
            assert!(result.0);
            assert_eq!(native.get(), before);
            assert_eq!(result.1.is_some(), index < CAPACITY);
            if let Some(token) = token {
                records.borrow_mut().release(token);
            }
        }
        assert_eq!(calls.get(), CAPACITY * 4);
        assert_eq!(records.borrow().magazines().count(), CAPACITY);
        assert_eq!(
            records.borrow().magazines().map(|m| m.rounds).sum::<i32>(),
            0
        );
    }
}

#[test]
fn automatic_nonempty_npc_or_unknown_accounting_never_duplicates_native_rounds() {
    for native_rounds in [Some(17), None] {
        assert_eq!(automatic_rounds(true, false, native_rounds), None);
        let native = Cell::new((17, 70));
        let result = dispatch_drop(
            None,
            true,
            || panic!("untracked needs no identity"),
            |_, _| panic!("untracked must not begin"),
            || true,
            |_, _| panic!("unproven native ammo must not be imported"),
        );
        assert_eq!(result, (true, None));
        assert_eq!(native.get(), (17, 70));
    }
    assert_eq!(automatic_rounds(false, true, Some(17)), None);
    assert_eq!(automatic_rounds(false, false, Some(0)), None);
}

#[test]
fn automatic_admission_failure_runs_native_but_manual_failure_refuses() {
    let mut records = Records::new();
    let token = records.reserve(key(0)).unwrap();
    let expected = ExpectedDrop {
        key: key(0),
        token,
        rounds: 0,
    };
    for automatic in [false, true] {
        for invalid_tag in [false, true] {
            let calls = Cell::new(0);
            let result = dispatch_drop(
                Some(expected),
                automatic,
                || {
                    Some((
                        key(0),
                        if invalid_tag {
                            WeaponData::NO_MAGAZINE_UNIQUE_TAG
                        } else {
                            1
                        },
                    ))
                },
                |_, _| false,
                || {
                    calls.set(calls.get() + 1);
                    true
                },
                |_, _| panic!("failed admission must not commit"),
            );
            assert_eq!(result, (automatic, None));
            assert_eq!(calls.get(), usize::from(automatic));
            assert!(records.matches(token, key(0)));
        }
    }
}

#[test]
fn unrelated_native_reentry_does_not_consume_manual_reservation() {
    let mut records = Records::new();
    let token = records.reserve(key(0)).unwrap();
    let expected = ExpectedDrop {
        key: key(0),
        token,
        rounds: 17,
    };
    let result = dispatch_drop(
        Some(expected),
        false,
        || Some((key(1), 1)),
        |_, _| panic!("unrelated native reentry must not begin the manual reservation"),
        || true,
        |_, _| panic!("unrelated reentry must not fill the manual result"),
    );
    assert_eq!(result, (true, None));
    assert!(records.matches(token, key(0)));
}

#[test]
fn more_manual_drops_than_capacity_refuse_before_engine_and_conserve_all_rounds() {
    let records = RefCell::new(Records::new());
    let calls = Cell::new(0);
    let spawned = Cell::new(0);
    let weapon_rounds = Cell::new(0);
    for index in 0..CAPACITY * 4 {
        let before = (17, 70, false, true, true);
        let weapon = Cell::new(before);
        let outcome = with_admission(
            || records.borrow_mut().reserve(key(index)),
            DetachOutcome::NoThrow,
            |token| {
                let expected = ExpectedDrop {
                    key: key(index),
                    token: *token,
                    rounds: 17,
                };
                let result = dispatch_drop(
                    Some(expected),
                    false,
                    || Some((key(index), 1)),
                    |e, k| records.borrow_mut().begin(e.token, k),
                    || {
                        calls.set(calls.get() + 1);
                        true
                    },
                    |e, _| {
                        records.borrow_mut().commit(
                            e.token,
                            captured(index, e.rounds),
                            Instant::now(),
                        )
                    },
                );
                let outcome = finish_detach(
                    result,
                    || spawned.set(spawned.get() + 1),
                    |retained| {
                        weapon.set((if retained { 0 } else { 17 }, 70, true, true, false));
                    },
                );
                records.borrow_mut().release(*token);
                outcome
            },
        );
        if index < CAPACITY {
            let DetachOutcome::Retained(magazine) = outcome else {
                panic!("admitted drop not retained");
            };
            assert!(records.borrow().contains(magazine));
            assert_eq!(magazine.rounds, 17);
            assert_eq!(weapon.get().0, 0);
        } else {
            assert_eq!(outcome, DetachOutcome::NoThrow);
            assert_eq!(weapon.get(), before);
        }
        weapon_rounds.set(weapon_rounds.get() + weapon.get().0);
    }
    let stored = records.borrow().magazines().map(|m| m.rounds).sum::<i32>();
    assert_eq!(calls.get(), CAPACITY);
    assert_eq!(spawned.get(), CAPACITY);
    assert_eq!(
        weapon_rounds.get() + stored + 70,
        CAPACITY as i32 * 4 * 17 + 70
    );
}

#[test]
fn manual_no_throw_and_unretained_results_preserve_native_accounting() {
    for thrown in [false, true] {
        let records = RefCell::new(Records::new());
        let before = (17, 70, false);
        let weapon = Cell::new(before);
        let spawns = Cell::new(0);
        let outcome = with_admission(
            || records.borrow_mut().reserve(key(0)),
            DetachOutcome::NoThrow,
            |token| {
                let expected = ExpectedDrop {
                    key: key(0),
                    token: *token,
                    rounds: 17,
                };
                let result = dispatch_drop(
                    Some(expected),
                    false,
                    || Some((key(0), 1)),
                    |e, k| records.borrow_mut().begin(e.token, k),
                    || thrown,
                    |_, _| None,
                );
                let outcome = finish_detach(
                    result,
                    || spawns.set(spawns.get() + 1),
                    |retained| {
                        assert!(!retained);
                        weapon.set((17, 70, true));
                    },
                );
                records.borrow_mut().release(*token);
                outcome
            },
        );
        assert_eq!(
            outcome,
            if thrown {
                DetachOutcome::DroppedUnretained
            } else {
                DetachOutcome::NoThrow
            }
        );
        assert_eq!(weapon.get(), if thrown { (17, 70, true) } else { before });
        assert_eq!(spawns.get(), usize::from(thrown));
        assert_eq!(records.borrow().magazines().count(), 0);
        assert!(records.borrow_mut().reserve(key(1)).is_some());
    }
}

#[test]
fn explicit_invalidation_clears_held_items_without_sightings() {
    let mut state = State {
        records: Records::new(),
        held: None,
        counts: LooseCounts::default(),
    };
    let now = Instant::now();
    let mut held = None;
    for index in 0..CAPACITY {
        let token = state.records.reserve(key(index)).unwrap();
        assert!(state.records.begin(token, key(index)));
        let magazine = state
            .records
            .commit(token, captured(index, 17), now)
            .unwrap();
        if index == 0 {
            held = Some(magazine);
        }
        state.records.release(token);
    }
    let held = held.unwrap();
    assert!(state.take(Hand::Left, held));
    let mut checked = 0;
    state.maintain(now + Duration::from_secs(60), 0.0, |_| {
        checked += 1;
        Maintenance::Gone
    });
    assert_eq!(checked, CAPACITY);
    assert_eq!(state.counts.in_world, 0);
    assert!(state.held.is_none());
    assert!(!state.take(Hand::Left, held));
    assert!(state.records.reserve(key(CAPACITY)).is_some());
}
