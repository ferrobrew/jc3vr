use std::{num::NonZeroUsize, time::Instant};

use super::*;
use crate::controllers::{
    frame::Hand,
    weapons::reload::loose::{
        Captured, LooseCounts,
        records::{DropKey, Records},
    },
};

struct FakeBodies {
    read: Result<(Vec3, Quat), BodyError>,
    write: Result<(), BodyError>,
    reads: usize,
    writes: usize,
}

impl Bodies for FakeBodies {
    fn pose(&mut self, _: Loose) -> Result<(Vec3, Quat), BodyError> {
        self.reads += 1;
        self.read
    }
    fn velocities(&mut self, _: Loose, _: Vec3, _: Vec3) -> Result<(), BodyError> {
        self.writes += 1;
        self.write
    }
    fn disable(&mut self, _: Loose) {}
}

fn state() -> (State, Loose) {
    let mut state = State {
        records: Records::new(),
        held: None,
        counts: LooseCounts::default(),
    };
    let key = DropKey { gun: 1, spare: 100 };
    let token = state.records.reserve(key).unwrap();
    assert!(state.records.begin(token, key));
    let magazine = state
        .records
        .commit(
            token,
            Captured {
                object: NonZeroUsize::new(100).unwrap(),
                body: 1,
                owner: NonZeroUsize::new(1000).unwrap(),
                tag: 1,
                rounds: 17,
            },
            Instant::now(),
        )
        .unwrap();
    state.records.release(token);
    assert!(state.take(Hand::Left, magazine));
    (state, magazine)
}

fn carry<'a>(config: &'a LooseMagazineConfig) -> Carry<'a> {
    Carry {
        top: Vec3::Y,
        axis: Vec3::Y,
        dt: 0.01,
        config,
    }
}

#[test]
fn reownership_from_carry_read_seat_read_or_velocity_write_is_sticky_between_maintenance_calls() {
    let config = LooseMagazineConfig::default();
    for operation in 0..3 {
        for error in [BodyError::NoBody, BodyError::Reowned] {
            let (mut state, magazine) = state();
            let mut bodies = FakeBodies {
                read: Ok((Vec3::ZERO, Quat::IDENTITY)),
                write: Ok(()),
                reads: 0,
                writes: 0,
            };
            if operation == 2 {
                bodies.write = Err(error);
            } else {
                bodies.read = Err(error);
            }
            if operation == 1 {
                assert_eq!(state.seat_with(&mut bodies), Operation::Deferred);
            } else {
                assert_eq!(
                    state.carry_with(carry(&config), &mut bodies),
                    Operation::Deferred
                );
            }
            assert!(state.records.contains(magazine));
            assert!(!state.records.usable(magazine));
            bodies.read = Ok((Vec3::ZERO, Quat::IDENTITY));
            bodies.write = Ok(());
            assert_eq!(
                state.observe(magazine, bodies.pose(magazine)),
                Operation::Deferred
            );
            assert_eq!(state.seat_with(&mut bodies), Operation::Deferred);
            assert_eq!(
                state.carry_with(carry(&config), &mut bodies),
                Operation::Deferred
            );
            assert!(state.held.is_some());
            assert_eq!(state.records.magazines().map(|m| m.rounds).sum::<i32>(), 17);
            state.held = None;
            assert!(!state.take(Hand::Left, magazine));
        }
    }
}

#[test]
fn temporary_pose_or_write_errors_defer_and_recover_without_consuming_rounds() {
    let config = LooseMagazineConfig::default();
    for error in [
        BodyError::NoPhysicsWorld,
        BodyError::NotDynamic,
        BodyError::BadMotion,
    ] {
        let (mut state, magazine) = state();
        let mut bodies = FakeBodies {
            read: Err(error),
            write: Ok(()),
            reads: 0,
            writes: 0,
        };
        assert_eq!(
            state.carry_with(carry(&config), &mut bodies),
            Operation::Deferred
        );
        assert_eq!(state.seat_with(&mut bodies), Operation::Deferred);
        assert!(state.records.usable(magazine));
        bodies.read = Ok((Vec3::ZERO, Quat::IDENTITY));
        bodies.write = Err(error);
        assert_eq!(
            state.carry_with(carry(&config), &mut bodies),
            Operation::Deferred
        );
        bodies.write = Ok(());
        assert_eq!(
            state.carry_with(carry(&config), &mut bodies),
            Operation::Ready(())
        );
        assert_eq!(state.seat_with(&mut bodies), Operation::Ready(magazine));
        assert_eq!(state.seat_with(&mut bodies), Operation::Gone);
    }
}

#[test]
fn a_late_observation_cannot_quarantine_the_replacement_generation() {
    let (mut state, old) = state();
    assert!(state.records.remove(old));
    state.held = None;
    let key = DropKey { gun: 1, spare: 100 };
    let token = state.records.reserve(key).unwrap();
    assert!(state.records.begin(token, key));
    let replacement = state
        .records
        .commit(
            token,
            Captured {
                object: old.object,
                body: old.body,
                owner: old.owner,
                tag: old.tag,
                rounds: old.rounds,
            },
            Instant::now(),
        )
        .unwrap();
    assert_eq!(
        state.observe(old, Err::<(), _>(BodyError::Reowned)),
        Operation::Gone
    );
    assert!(state.records.usable(replacement));
}
