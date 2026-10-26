use std::{cell::Cell, num::NonZeroUsize};

use super::*;
use crate::controllers::weapons::reload::{
    engine::{DetachOutcome, finish_detach},
    loose::Loose,
};

#[test]
fn runtime_hand_mapping_follows_the_gun_and_standdown_releases_gating() {
    let mut runtime = super::Runtime::new(
        Ammo::adopt(
            NativeAmmo {
                total: 1,
                out: true,
            },
            [Chamber::Chambered, Chamber::Empty],
        ),
        [Some((0, false)), None],
    );
    runtime.remap([None, Some((0, false))]);
    assert_eq!(runtime.shot(0, 0), None);
    assert!(!runtime.holds_fire(1));
    assert_eq!(runtime.shot(1, 0), Some(0));
    assert!(runtime.holds_fire(1));
    let spent = runtime.ammo;
    runtime.suspend();
    assert!(!runtime.holds_fire(1));
    assert_eq!(runtime.shot(1, 0), None);
    assert_eq!(runtime.ammo, spent);
    runtime.remap([Some((0, false)), None]);
    assert!(runtime.holds_fire(0));
}

#[test]
fn replacement_runtime_adopts_only_its_native_rounds() {
    let old = super::Runtime::new(
        Ammo::adopt(
            NativeAmmo {
                total: 1,
                out: true,
            },
            [Chamber::Chambered, Chamber::Empty],
        ),
        [Some((0, false)), None],
    );
    let replacement = super::Runtime::new(
        Ammo::adopt(
            NativeAmmo {
                total: 0,
                out: false,
            },
            old.ammo.chambers,
        ),
        [Some((0, false)), None],
    );
    assert_eq!(replacement.ammo.total(), 0);
    assert!(replacement.holds_fire(0));
    assert_eq!(old.ammo.total(), 1);
}

#[test]
fn observed_disabled_policy_releases_empty_chamber_gates_with_loaded_rounds() {
    for stand_down in [false, true] {
        let native = NativeAmmo {
            total: 5,
            out: false,
        };
        let mut runtime = super::Runtime::new(
            Ammo::adopt(native, [Chamber::Empty; 2]),
            [Some((0, true)), None],
        );
        let mut observation = Observation {
            native,
            chambers: [Chamber::Chambered; 2],
            topology: [true, false],
            guns: [Some((0, true)), None],
            active: true,
        };
        assert!(runtime.observe(observation).is_some());
        assert!(runtime.holds_fire(0));
        if stand_down {
            runtime.suspend();
        }
        let partition = runtime.ammo;
        observation.active = false;
        assert!(runtime.observe(observation).is_none());
        assert!(!runtime.holds_fire(0));
        assert_eq!(runtime.guns, [None; 2]);
        assert_eq!(runtime.shot(0, 4), None);
        assert_eq!(runtime.ammo, partition);
        assert_eq!(runtime.ammo.magazine, 5);
    }
}

#[test]
fn observed_topology_removes_hidden_chamber_without_using_stale_records() {
    let mut runtime = super::Runtime::new(
        Ammo::adopt(
            NativeAmmo {
                total: 8,
                out: false,
            },
            [Chamber::Chambered; 2],
        ),
        [Some((0, false)), Some((1, false))],
    );
    let observation = Observation {
        native: NativeAmmo {
            total: 8,
            out: false,
        },
        chambers: [Chamber::Chambered; 2],
        topology: [true, false],
        guns: [Some((0, false)), None],
        active: true,
    };
    let partition = runtime.observe(observation).unwrap();
    assert_eq!(partition.chambers, [Chamber::Chambered, Chamber::Empty]);
    assert_eq!((partition.magazine, partition.total()), (7, 8));
    runtime.ammo.eject(0);
    runtime.observe(Observation {
        native: NativeAmmo {
            total: 7,
            out: false,
        },
        ..observation
    });
    assert_eq!(runtime.ammo.chambers, [Chamber::Empty; 2]);
}

#[test]
fn observed_pair_allocation_gives_each_single_one_chamber_and_magazine_capacity() {
    for total in [24, 26, 30] {
        let native = NativeAmmo { total, out: false };
        let mut slot = super::Runtime::new(
            Ammo::adopt(native, [Chamber::Chambered; 2]),
            [Some((0, false)), Some((1, false))],
        );
        slot.observe(Observation {
            native,
            chambers: [Chamber::Chambered; 2],
            topology: [true; 2],
            guns: [Some((0, false)), Some((1, false))],
            active: true,
        });
        let (dealt, mut twin) = slot.split_pair(native, 24);
        assert_eq!(dealt.slot + dealt.twin + dealt.over, total);
        for (runtime, allocated) in [(&mut slot, dealt.slot), (&mut twin, dealt.twin)] {
            let partition = runtime
                .observe(Observation {
                    native: NativeAmmo {
                        total: allocated,
                        out: false,
                    },
                    chambers: [Chamber::Chambered; 2],
                    topology: [true, false],
                    guns: [Some((0, false)), None],
                    active: true,
                })
                .unwrap();
            assert_eq!(partition.chambers, [Chamber::Chambered, Chamber::Empty]);
            assert_eq!(partition.chamber_rounds(), 1);
            assert!(partition.magazine <= 12);
            assert!(partition.total() <= 13);
            let retained = partition.magazine;
            runtime.ammo.detach(true);
            assert_eq!(runtime.ammo.total(), 1);
            assert_eq!(retained + runtime.ammo.total(), allocated);
        }
    }
}

fn magazine(rounds: i32) -> Loose {
    Loose::test(
        NonZeroUsize::new(1).unwrap(),
        1,
        NonZeroUsize::new(1).unwrap(),
        1,
        rounds,
    )
}

struct Runtime {
    native: NativeAmmo,
    ammo: Ammo,
    reserve: i32,
    fired: u32,
    spawned: Cell<u32>,
}

impl Runtime {
    fn loaded(total: i32, ready: bool) -> Self {
        let native = NativeAmmo { total, out: false };
        Self {
            native,
            ammo: Ammo::adopt(
                native,
                [
                    if ready {
                        Chamber::Chambered
                    } else {
                        Chamber::Empty
                    },
                    Chamber::Empty,
                ],
            ),
            reserve: 20,
            fired: 0,
            spawned: Cell::new(0),
        }
    }

    fn detach(&mut self, thrown: bool, retained: bool) -> DetachOutcome {
        let magazine = magazine(self.ammo.magazine);
        let retained = retained.then_some(magazine);
        finish_detach(
            (thrown, retained),
            || self.spawned.set(self.spawned.get() + 1),
            |retained| {
                self.ammo.detach(retained);
                self.native = NativeAmmo {
                    total: self.ammo.total(),
                    out: true,
                };
            },
        )
    }

    fn insert(&mut self, rounds: i32) {
        self.reserve += self.ammo.insert(rounds);
        self.native = NativeAmmo {
            total: self.ammo.total(),
            out: false,
        };
    }

    fn fire(&mut self, deferred_native_debit: bool, manual: bool) -> bool {
        if self.ammo.chambers[0] == Chamber::Empty {
            return false;
        }
        self.fired += 1;
        let native_left = self.native.total - i32::from(!deferred_native_debit);
        self.native.total = self.ammo.shot(0, native_left, manual);
        true
    }

    fn rear(&mut self) {
        self.ammo.eject(0);
        self.native.total = self.ammo.total();
    }

    fn home(&mut self) {
        self.ammo.feed(0);
        assert_eq!(self.native.total, self.ammo.total());
    }

    fn native_reload(&mut self, capacity: i32) {
        let magazine_only = self.ammo.reload_start();
        let new_magazine = capacity.min(magazine_only + self.reserve);
        self.reserve += magazine_only - new_magazine;
        self.native = NativeAmmo {
            total: self.ammo.reload_finish(new_magazine),
            out: false,
        };
    }
}

#[test]
fn chambered_loaded_extraction_transfers_only_magazine_owned_rounds() {
    let mut runtime = Runtime::loaded(8, true);
    assert_eq!(runtime.ammo.magazine, 7);
    assert_eq!(
        runtime.detach(true, true),
        DetachOutcome::Retained(magazine(7))
    );
    assert_eq!(
        runtime.native,
        NativeAmmo {
            total: 1,
            out: true
        }
    );
    assert_eq!(runtime.ammo.chambers[0], Chamber::Chambered);
    assert_eq!(runtime.spawned.get(), 1);
    assert_eq!(runtime.reserve, 20);
}

#[test]
fn empty_and_dry_extraction_never_materialize_a_chamber() {
    for total in [0, 5] {
        let mut runtime = Runtime::loaded(total, false);
        assert_eq!(
            runtime.detach(true, true),
            DetachOutcome::Retained(magazine(total))
        );
        assert_eq!(runtime.native.total, 0);
        assert!(!runtime.fire(false, false));
        runtime.home();
        assert_eq!(runtime.ammo.chambers[0], Chamber::Empty);
    }
}

#[test]
fn magazine_out_fires_once_even_when_native_burst_debit_is_deferred() {
    for retained in [false, true] {
        for deferred in [false, true] {
            let mut runtime = Runtime::loaded(8, true);
            runtime.detach(true, retained);
            assert!(runtime.fire(deferred, false));
            assert!(!runtime.fire(deferred, false));
            assert_eq!(runtime.fired, 1);
            assert_eq!(runtime.native.total, if retained { 0 } else { 7 });
            assert_eq!(runtime.ammo.chambers[0], Chamber::Empty);
            assert_eq!(runtime.ammo.magazine, 0);
        }
    }
}

#[test]
fn inserting_different_magazines_preserves_chamber_without_refunding_it() {
    let mut runtime = Runtime::loaded(8, true);
    runtime.detach(true, true);
    runtime.insert(3);
    assert_eq!(
        (runtime.native.total, runtime.ammo.magazine, runtime.reserve),
        (4, 3, 20)
    );
    runtime.detach(true, true);
    runtime.insert(7);
    assert_eq!(
        (runtime.native.total, runtime.ammo.magazine, runtime.reserve),
        (8, 7, 20)
    );
    assert!(runtime.fire(false, false));
    assert_eq!((runtime.native.total, runtime.ammo.magazine), (7, 6));
}

#[test]
fn unretained_button_or_belt_eject_returns_only_removed_magazine_rounds() {
    let mut runtime = Runtime::loaded(8, true);
    assert_eq!(
        runtime.detach(true, false),
        DetachOutcome::DroppedUnretained
    );
    assert_eq!((runtime.ammo.detached, runtime.native.total), (7, 8));
    runtime.native_reload(10);
    runtime.home();
    assert_eq!(
        (runtime.ammo.magazine, runtime.native.total, runtime.reserve),
        (10, 11, 17)
    );
}

#[test]
fn repeated_rack_events_eject_and_feed_once_with_or_without_a_magazine() {
    let mut runtime = Runtime::loaded(4, true);
    runtime.rear();
    runtime.rear();
    assert_eq!(runtime.native.total, 3);
    runtime.home();
    runtime.home();
    assert_eq!(
        (runtime.ammo.magazine, runtime.ammo.chamber_rounds()),
        (2, 1)
    );
    runtime.detach(true, true);
    runtime.rear();
    runtime.rear();
    runtime.home();
    assert_eq!(runtime.native.total, 0);
    assert_eq!(runtime.ammo.chambers[0], Chamber::Empty);
}

#[test]
fn manual_shot_requires_feed_and_does_not_consume_an_extra_round() {
    let mut runtime = Runtime::loaded(4, true);
    assert!(runtime.fire(false, true));
    assert_eq!((runtime.native.total, runtime.ammo.magazine), (3, 3));
    assert!(!runtime.fire(false, true));
    runtime.rear();
    runtime.home();
    assert_eq!((runtime.native.total, runtime.ammo.magazine), (3, 2));
    assert!(runtime.fire(false, true));
    assert_eq!(runtime.native.total, 2);
}

#[test]
fn aborted_detach_commits_neither_ammo_nor_chamber() {
    let mut runtime = Runtime::loaded(8, true);
    let before = runtime.ammo;
    assert_eq!(runtime.detach(false, true), DetachOutcome::NoThrow);
    assert_eq!(runtime.ammo, before);
    assert_eq!(
        runtime.native,
        NativeAmmo {
            total: 8,
            out: false
        }
    );
    assert_eq!(runtime.spawned.get(), 0);
}

#[test]
fn drop_switch_and_standdown_reconcile_without_restoring_a_spent_chamber() {
    let mut runtime = Runtime::loaded(8, true);
    runtime.detach(true, true);
    let switched = runtime.ammo;
    runtime.ammo.reconcile(runtime.native);
    assert_eq!(runtime.ammo, switched);
    assert!(runtime.fire(false, false));
    runtime.ammo.reconcile(runtime.native);
    assert_eq!(runtime.ammo.chambers[0], Chamber::Empty);
    let replacement = Ammo::adopt(
        NativeAmmo {
            total: 5,
            out: false,
        },
        [Chamber::Empty; 2],
    );
    assert_eq!((replacement.magazine, replacement.chamber_rounds()), (5, 0));
    assert_eq!(runtime.native.total, runtime.ammo.total());
}
