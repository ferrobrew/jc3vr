use std::{num::NonZeroUsize, time::Duration};

use super::*;
use crate::controllers::{
    frame::Hand,
    physical_hands::BodyError,
    weapons::reload::loose::{LooseCounts, State, body_status},
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

fn commit(records: &mut Records, index: usize, rounds: i32, now: Instant) -> (Token, Loose) {
    let token = records.reserve(key(index)).unwrap();
    assert!(records.begin(token, key(index)));
    (
        token,
        records.commit(token, captured(index, rounds), now).unwrap(),
    )
}

#[test]
fn full_nonempty_capacity_cannot_append_or_erase_rounds() {
    let mut records = Records::new();
    let now = Instant::now();
    for index in 0..CAPACITY {
        let (token, _) = commit(&mut records, index, 17, now);
        records.release(token);
    }
    for index in CAPACITY..CAPACITY * 4 {
        assert!(records.reserve(key(index)).is_none());
    }
    records.maintain(now + Duration::from_secs(60), 0.0, None, |_| {
        Maintenance::Live
    });
    assert_eq!(records.magazines().count(), CAPACITY);
    assert_eq!(
        records.magazines().map(|m| m.rounds).sum::<i32>(),
        CAPACITY as i32 * 17
    );
}

#[test]
fn reservations_executing_results_and_held_items_share_the_cap() {
    let mut records = Records::new();
    let now = Instant::now();
    let (held_token, held) = commit(&mut records, 0, 0, now);
    records.release(held_token);
    let (result, unread) = commit(&mut records, 1, 0, now);
    let mut reserved = Vec::new();
    for index in 2..CAPACITY {
        reserved.push(records.reserve(key(index)).unwrap());
    }
    assert!(records.begin(reserved[0], key(2)));
    records.maintain(
        now + Duration::from_secs(60),
        0.0,
        Some(held.handle),
        |_| Maintenance::Live,
    );
    assert!(records.contains(held));
    assert!(records.contains(unread));
    assert!(records.reserve(key(CAPACITY)).is_none());
    records.release(result);
    records.maintain(
        now + Duration::from_secs(60),
        0.0,
        Some(held.handle),
        |_| Maintenance::Live,
    );
    assert!(records.contains(held));
    assert!(!records.contains(unread));
    assert!(records.reserve(key(CAPACITY)).is_some());
}

#[test]
fn cancel_releases_unused_capacity_but_keeps_committed_world_rounds_once() {
    let mut records = Records::new();
    let now = Instant::now();
    let pending = records.reserve(key(0)).unwrap();
    records.release(pending);
    assert!(!records.begin(pending, key(0)));
    let (result, magazine) = commit(&mut records, 0, 17, now);
    assert!(!records.begin(result, key(0)));
    assert!(records.commit(result, captured(0, 17), now).is_none());
    records.release(result);
    records.release(result);
    assert!(records.remove(magazine));
    assert!(!records.remove(magazine));
}

#[test]
fn stale_copied_result_cannot_take_or_remove_identical_native_fields_in_a_new_generation() {
    let now = Instant::now();
    let mut state = State {
        records: Records::new(),
        held: None,
        counts: LooseCounts::default(),
    };
    let (old_token, old_result) = commit(&mut state.records, 0, 17, now);
    assert!(state.take(Hand::Left, old_result));
    // Gone requires explicit lifetime proof; a body read error never produces this status.
    state.maintain(now, 60.0, |_| Maintenance::Gone);
    assert!(state.held.is_none());
    let (_, replacement) = commit(&mut state.records, 0, 17, now);
    assert_ne!(old_result.handle, replacement.handle);
    assert_eq!(
        (
            old_result.object,
            old_result.body,
            old_result.owner,
            old_result.tag,
            old_result.rounds
        ),
        (
            replacement.object,
            replacement.body,
            replacement.owner,
            replacement.tag,
            replacement.rounds
        )
    );
    state.records.release(old_token);
    assert!(!state.records.contains(old_result));
    assert!(!state.take(Hand::Left, old_result));
    assert!(!state.records.remove(old_result));
    assert!(state.records.contains(replacement));
    assert!(state.take(Hand::Left, replacement));
    assert!(state.records.remove(replacement));
}

#[test]
fn every_body_error_preserves_held_unread_rounds_and_capacity() {
    for error in [
        BodyError::NoPhysicsWorld,
        BodyError::NoBody,
        BodyError::NotDynamic,
        BodyError::Reowned,
        BodyError::BadMotion,
    ] {
        let now = Instant::now();
        let mut state = State {
            records: Records::new(),
            held: None,
            counts: LooseCounts::default(),
        };
        let mut first = None;
        for index in 0..CAPACITY {
            let (_, m) = commit(&mut state.records, index, 17, now);
            if index == 0 {
                first = Some(m);
            }
        }
        let held = first.unwrap();
        assert!(state.take(Hand::Left, held));
        state.maintain(now + Duration::from_secs(60), 0.0, |_| {
            body_status(Err(error))
        });
        assert_eq!(state.held.unwrap().magazine, held);
        assert_eq!(
            state.records.magazines().map(|m| m.rounds).sum::<i32>(),
            CAPACITY as i32 * 17
        );
        assert!(state.records.reserve(key(CAPACITY)).is_none());
        state.maintain(now, 60.0, |_| Maintenance::Live);
        let lost = matches!(error, BodyError::NoBody | BodyError::Reowned);
        assert_eq!(state.records.usable(held), !lost);
        if lost {
            assert!(!state.records.remove(held));
        }
    }
}

#[test]
fn unavailable_empty_records_do_not_expire_and_confirmed_invalidation_checks_only_world_slots() {
    let now = Instant::now();
    let mut records = Records::new();
    let (token, empty) = commit(&mut records, 0, 0, now);
    records.release(token);
    records.maintain(now + Duration::from_secs(60), 0.0, None, |_| {
        body_status(Err(BodyError::NotDynamic))
    });
    assert!(records.contains(empty));
    let pending = records.reserve(key(1)).unwrap();
    let mut checked = 0;
    records.maintain(now, 60.0, None, |_| {
        checked += 1;
        Maintenance::Gone
    });
    assert_eq!(checked, 1);
    assert!(!records.contains(empty));
    assert!(records.matches(pending, key(1)));
}

#[test]
fn generation_exhaustion_refuses_without_reusing_a_handle_or_mutating_existing_records() {
    let mut records = Records::new();
    records.generation = u64::MAX - 1;
    let (last, magazine) = commit(&mut records, 0, 17, Instant::now());
    assert_eq!(last.generation, u64::MAX);
    assert!(records.reserve(key(1)).is_none());
    assert!(records.contains(magazine));
    records.release(last);
    assert!(records.remove(magazine));
    assert!(records.reserve(key(0)).is_none());
}
