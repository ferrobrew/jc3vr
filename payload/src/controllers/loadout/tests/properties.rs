//! The table's invariants under arbitrary sequences of events: every event applies whole or not at
//! all, the transitions say what the table became, the player's moves touch only their item, and
//! the spawn reset never changes who is who.

use pretty_assertions::assert_eq;
use proptest::{collection::vec, prelude::*};

use super::fixtures::*;
use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Awaiting, Binding},
        event::Event,
        item::{ItemId, ItemKind, WeaponType},
        reset::{CacheKind, ResetKind, SlotObject},
        socket::{BodyLayout, Socket},
        table::{ItemView, Loadout, Place},
        transition::{
            Change, Give, GiveSource, LostWhy, Release, Removal, RemovalWhy, Take, TakeSource,
            TransitionError,
        },
    },
};

/// An event chosen by index, made concrete against the table it is applied to.
#[derive(Debug, Clone)]
enum Op {
    Take {
        hand: usize,
        object: usize,
        kind: usize,
        source: usize,
    },
    Dock {
        hand: usize,
        socket: usize,
    },
    Draw {
        socket: usize,
        hand: usize,
    },
    HandOver {
        hand: usize,
    },
    LetGo {
        hand: usize,
        release: usize,
    },
    Lost {
        object: usize,
        why: usize,
    },
    Give {
        object: usize,
        kind: usize,
        source: usize,
    },
    Spawned {
        row: usize,
        object: usize,
    },
    Remove {
        target: usize,
        value: usize,
        why: usize,
    },
    Store,
    Cache {
        kind: usize,
    },
    Restore {
        kind: usize,
        slots: Vec<(usize, usize)>,
    },
    ResetBegan,
    ResetCompleted {
        kind: usize,
        slots: Vec<(usize, usize)>,
    },
    SetBody {
        dominant: usize,
        back: u8,
        hips: u8,
        chest: bool,
    },
}

/// How many made-up objects the sequences draw from, so that objects repeat.
const OBJECTS: usize = 12;

fn kinds() -> [ItemKind; 8] {
    [
        weapon(a_pistol()),
        weapon(an_smg()),
        weapon(a_rifle()),
        weapon(a_sniper()),
        weapon(a_launcher()),
        ItemKind::Prop,
        ItemKind::Grenade,
        ItemKind::Charge,
    ]
}

fn weapons() -> [WeaponType; 5] {
    [a_pistol(), an_smg(), a_rifle(), a_sniper(), a_launcher()]
}

fn op() -> impl Strategy<Value = Op> {
    let hand = 0..2usize;
    let object = 1..=OBJECTS;
    let slots = || vec((1..=OBJECTS, 0..5usize), 0..4);
    prop_oneof![
        4 => (hand.clone(), object.clone(), 0..8usize, 0..TakeSource::ALL.len())
            .prop_map(|(hand, object, kind, source)| Op::Take { hand, object, kind, source }),
        3 => (hand.clone(), 0..Socket::COUNT).prop_map(|(hand, socket)| Op::Dock { hand, socket }),
        3 => (0..Socket::COUNT, hand.clone()).prop_map(|(socket, hand)| Op::Draw { socket, hand }),
        2 => hand.clone().prop_map(|hand| Op::HandOver { hand }),
        3 => (hand.clone(), 0..Release::ALL.len())
            .prop_map(|(hand, release)| Op::LetGo { hand, release }),
        2 => (object.clone(), 0..LostWhy::ALL.len()).prop_map(|(object, why)| Op::Lost { object, why }),
        2 => (object.clone(), 0..5usize, 0..GiveSource::ALL.len())
            .prop_map(|(object, kind, source)| Op::Give { object, kind, source }),
        1 => (0..8usize, object.clone()).prop_map(|(row, object)| Op::Spawned { row, object }),
        2 => (0..3usize, 0..OBJECTS, 0..RemovalWhy::ALL.len())
            .prop_map(|(target, value, why)| Op::Remove { target, value, why }),
        1 => Just(Op::Store),
        1 => (0..CacheKind::COUNT).prop_map(|kind| Op::Cache { kind }),
        1 => (0..CacheKind::COUNT, slots()).prop_map(|(kind, slots)| Op::Restore { kind, slots }),
        1 => Just(Op::ResetBegan),
        1 => (0..ResetKind::ALL.len(), slots())
            .prop_map(|(kind, slots)| Op::ResetCompleted { kind, slots }),
        1 => (hand, 0..4u8, 0..3u8, any::<bool>())
            .prop_map(|(dominant, back, hips, chest)| Op::SetBody { dominant, back, hips, chest }),
    ]
}

/// What applying `op` to `table` is: an event to fold, or a change of the body.
enum Applied {
    Event(Event),
    Body(BodyLayout),
}

fn concrete(op: &Op, table: &Loadout) -> Applied {
    let rows: Vec<ItemView> = table.items().collect();
    let slot_objects = |slots: &[(usize, usize)]| -> Vec<SlotObject> {
        slots
            .iter()
            .map(|&(n, w)| SlotObject {
                object: object(n),
                weapon: weapons()[w],
            })
            .collect()
    };
    Applied::Event(match op.clone() {
        Op::Take {
            hand,
            object: n,
            kind,
            source,
        } => Event::Take(Take {
            hand: Hand::BOTH[hand],
            object: object(n),
            kind: kinds()[kind],
            source: TakeSource::ALL[source],
        }),
        Op::Dock { hand, socket } => Event::Dock {
            hand: Hand::BOTH[hand],
            socket: Socket::ALL[socket],
        },
        Op::Draw { socket, hand } => Event::Draw {
            socket: Socket::ALL[socket],
            hand: Hand::BOTH[hand],
        },
        Op::HandOver { hand } => Event::HandOver {
            from: Hand::BOTH[hand],
        },
        Op::LetGo { hand, release } => Event::LetGo {
            hand: Hand::BOTH[hand],
            release: Release::ALL[release],
        },
        Op::Lost { object: n, why } => Event::Lost {
            object: object(n),
            why: LostWhy::ALL[why],
        },
        Op::Give {
            object: n,
            kind,
            source,
        } => Event::Given(Give {
            object: object(n),
            weapon: weapons()[kind],
            source: GiveSource::ALL[source],
        }),
        Op::Spawned { row, object: n } => match rows.get(row) {
            Some(item) => Event::Spawned {
                item: item.id,
                object: object(n),
            },
            None => Event::StoreImminent,
        },
        Op::Remove { target, value, why } => Event::Removed {
            target: match target {
                0 => Removal::Object(object(value + 1)),
                1 => match rows.get(value) {
                    Some(item) => Removal::Item(item.id),
                    None => Removal::AllWeapons,
                },
                _ => Removal::AllWeapons,
            },
            why: RemovalWhy::ALL[why],
        },
        Op::Store => Event::StoreImminent,
        Op::Cache { kind } => Event::Cached(CacheKind::ALL[kind]),
        Op::Restore { kind, slots } => Event::CacheRestored {
            cache: CacheKind::ALL[kind],
            slots: slot_objects(&slots),
        },
        Op::ResetBegan => Event::ResetBegan,
        Op::ResetCompleted { kind, slots } => Event::ResetCompleted {
            kind: ResetKind::ALL[kind],
            slots: slot_objects(&slots),
        },
        Op::SetBody {
            dominant,
            back,
            hips,
            chest,
        } => return Applied::Body(BodyLayout::new(Hand::BOTH[dominant], back, hips, chest)),
    })
}

/// Whether `event` is one of the player's own moves, which touch only the item they name.
fn players_move(event: &Event) -> bool {
    matches!(
        event,
        Event::Take(_)
            | Event::Dock { .. }
            | Event::Draw { .. }
            | Event::HandOver { .. }
            | Event::LetGo { .. }
    )
}

/// Apply `ops` one at a time, checking every property after each.
fn run(ops: &[Op]) -> Result<Loadout, TestCaseError> {
    let mut t = table();
    for op in ops {
        let before = t.snapshot();
        let caches: Vec<CacheKind> = t.caches_held().collect();
        let event = match concrete(op, &t) {
            Applied::Event(event) => event,
            Applied::Body(body) => {
                t.set_body(body);
                prop_assert_eq!(t.items().collect::<Vec<_>>(), before.items.clone());
                check_invariants(&t);
                continue;
            }
        };
        let player = players_move(&event);
        match t.fold(event.clone()) {
            Ok(folded) => {
                check_invariants(&t);
                let after: Vec<ItemView> = t.items().collect();
                for (id, end) in ends(&folded.transitions) {
                    prop_assert_eq!(place(&t, id), end, "{:?} said {} ends elsewhere", event, id);
                }
                let entered = folded
                    .transitions
                    .iter()
                    .filter(|tr| matches!(tr.change, Change::Entered { .. }))
                    .count();
                let left = folded
                    .transitions
                    .iter()
                    .filter(|tr| matches!(tr.change, Change::Left { .. }))
                    .count();
                prop_assert_eq!(after.len() + left, before.items.len() + entered);
                if player {
                    prop_assert!(
                        folded.transitions.len() <= 1,
                        "{:?} moved more than its item",
                        event
                    );
                    let moved: Vec<ItemId> = folded.transitions.iter().map(|tr| tr.item).collect();
                    for item in &before.items {
                        if !moved.contains(&item.id) {
                            prop_assert_eq!(
                                view(&t, item.id),
                                Some(*item),
                                "{:?} touched another item",
                                event
                            );
                        }
                    }
                }
            }
            Err(error) => {
                prop_assert_eq!(
                    &t.snapshot(),
                    &before,
                    "{:?} was refused ({}) but changed the table",
                    event,
                    error
                );
                prop_assert_eq!(t.caches_held().collect::<Vec<_>>(), caches);
            }
        }
    }
    Ok(t)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn every_event_keeps_the_invariants_and_applies_whole_or_not_at_all(
        ops in vec(op(), 1..60)
    ) {
        run(&ops)?;
    }

    #[test]
    fn the_spawn_reset_never_changes_who_is_who_or_where(
        ops in vec(op(), 0..40),
        respawn in any::<bool>(),
    ) {
        let mut t = run(&ops)?;
        t.begin_reset();
        let before: Vec<ItemView> = t
            .items()
            .filter(|item| !matches!(item.place, Place::World(_)))
            .collect();
        // The game puts a new object of each waiting item's type in its slot, as the reset does.
        let slots: Vec<SlotObject> = before
            .iter()
            .filter_map(|item| match (item.binding, item.kind.weapon()) {
                (Binding::Awaiting(Awaiting::Slot(_)), Some(weapon)) => Some(weapon),
                _ => None,
            })
            .enumerate()
            .map(|(n, weapon)| SlotObject { object: object(OBJECTS + 100 + n), weapon })
            .collect();
        let kind = if respawn { ResetKind::Respawn } else { ResetKind::Travel };
        let done = t.complete_reset(kind, &slots);
        prop_assert!(done.is_ok(), "{:?}", done.err());
        check_invariants(&t);
        for item in &before {
            let now = view(&t, item.id);
            match item.kind {
                // A respawn drops the props and the explosives in the hands.
                ItemKind::Prop | ItemKind::Grenade | ItemKind::Charge if respawn
                    && matches!(item.place, Place::Hand(_)) => prop_assert_eq!(now, None),
                _ => {
                    let now = now.ok_or_else(|| TestCaseError::fail(format!("{} was lost", item.id)))?;
                    prop_assert!(matches!(now.binding, Binding::Live(_) | Binding::Awaiting(Awaiting::Respawn)));
                    if !respawn {
                        prop_assert_eq!(now.place, item.place);
                    } else {
                        prop_assert!(!matches!(now.place, Place::Hand(_)), "the hands come back empty");
                    }
                }
            }
        }
    }
}

#[test]
fn a_refusal_is_reported_rather_than_panicking_on_an_empty_table() {
    let mut t = table();
    assert_eq!(
        t.fold(Event::HandOver { from: Hand::Left }),
        Err(TransitionError::HandEmpty(Hand::Left))
    );
    assert_eq!(
        t.fold(Event::ResetBegan).map(|f| f.transitions),
        Ok(Vec::new())
    );
}
