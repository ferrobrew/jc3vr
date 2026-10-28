use std::num::{NonZeroU64, NonZeroUsize};

use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};
use pretty_assertions::assert_eq;

use super::*;
use crate::controllers::loadout::{Departure, item::WeaponType, safe_items::ItemLeaseId};

fn object(n: usize) -> ObjectAddress {
    ObjectAddress::new(NonZeroUsize::MIN.saturating_add(n * 0x10))
}

fn rifle() -> SlotObject {
    SlotObject {
        object: object(1),
        weapon: WeaponType::new(
            0x301,
            WeaponSlot::E_SLOT_TWO_HANDED,
            WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE as i32,
        )
        .unwrap(),
    }
}

fn pistol() -> SlotObject {
    SlotObject {
        object: object(2),
        weapon: WeaponType::new(
            0x302,
            WeaponSlot::E_SLOT_DUAL_WIELD,
            WeaponClass::E_WEAPON_CLASS_PISTOL as i32,
        )
        .unwrap(),
    }
}

fn held(slot: SlotObject) -> Held {
    Held {
        object: slot.object,
        weapon: slot.weapon,
    }
}

/// A running table started from `slots`, as a load does.
fn started(slots: &[SlotObject]) -> Live {
    let mut live = Live::new();
    live.started = true;
    live.fold(Event::ResetCompleted {
        kind: ResetKind::Load,
        slots: slots.to_vec(),
    });
    live
}

fn tick<'a>(
    slots: &'a [SlotObject],
    wielded: Option<SlotObject>,
    armed: Option<(Hand, SlotObject)>,
) -> Tick<'a> {
    Tick {
        slots,
        carried: &[],
        wielded: wielded.map(held),
        armed: Hand::BOTH.map(|h| {
            armed
                .filter(|(hand, _)| *hand == h)
                .map(|(_, slot)| slot.object)
        }),
        fates: &[],
        others: &NO_OTHERS,
    }
}

const NO_OTHERS: HeldOthers = HeldOthers {
    hands: [None; 2],
    thrown: [false; 2],
};

/// Draw `slot`'s weapon off its socket into `hand`, as a take of the second weapon does.
fn draw(live: &mut Live, hand: Hand, slot: SlotObject) {
    let event = hands::arrival(&live.table, hand, held(slot), TakeSource::World).unwrap();
    live.fold(event);
}

#[test]
fn native_hand_events_cannot_bypass_safe_item_moves_or_release() {
    let mut live = started(&[]);
    let safe = live
        .table
        .admit_safe_item(ItemLeaseId::new(NonZeroU64::MIN), Hand::Left)
        .unwrap();
    let before = live.table.snapshot();
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
    ] {
        live.fold(event);
        assert_eq!(live.table.snapshot(), before);
    }
    assert_eq!(live.refusals, 3);
    let stowed = live
        .table
        .move_safe_item(safe, Place::Socket(Socket::Chest))
        .unwrap();
    let before = live.table.snapshot();
    live.fold(Event::Draw {
        socket: Socket::Chest,
        hand: Hand::Right,
    });
    assert_eq!(live.table.snapshot(), before);
    assert_eq!(live.table.safe_item(safe.item), Some(stowed));
    assert_eq!(live.refusals, 4);
}

#[test]
fn safe_hands_survive_native_observation_and_block_native_weapon_arrival() {
    let mut live = started(&[]);
    let left = live
        .table
        .admit_safe_item(ItemLeaseId::new(NonZeroU64::MIN), Hand::Left)
        .unwrap();
    let right = live
        .table
        .admit_safe_item(
            ItemLeaseId::new(NonZeroU64::MIN.saturating_add(1)),
            Hand::Right,
        )
        .unwrap();
    let before = live.table.snapshot();
    for others in [
        NO_OTHERS,
        HeldOthers {
            hands: [Some(HeldOther {
                object: object(7),
                kind: ItemKind::Grenade,
                source: TakeSource::Pouch,
            }); 2],
            thrown: [true; 2],
        },
    ] {
        live.sync_others(&others);
        assert_eq!(live.table.snapshot(), before);
    }
    live.pending = Some(Pending::new(Hand::Left, Expect::Draw, Some(rifle().object)));
    assert_eq!(live.land(held(rifle())), None);
    assert!(live.pending.is_some());
    assert_eq!(live.table.safe_item(left.item), Some(left));
    assert_eq!(live.table.safe_item(right.item), Some(right));
    live.table
        .move_safe_item(left, Place::World(Departure::Dropped))
        .unwrap();
    assert_eq!(live.land(held(rifle())).unwrap().hand, Hand::Left);
    assert_eq!(live.hand_holding(rifle().object), Some(Hand::Left));
    assert_eq!(live.table.safe_item(right.item), Some(right));
}

#[test]
fn a_drawn_weapon_lands_in_the_hand_that_expected_it() {
    let slots = [rifle()];
    let mut live = started(&slots);
    live.pending = Some(Pending::new(Hand::Left, Expect::Draw, None));
    let arrival = live.hand_moves(&tick(&slots, Some(rifle()), None));
    assert_eq!(
        arrival,
        Some(Arrival {
            hand: Hand::Left,
            expect: Some(Expect::Draw)
        })
    );
    assert_eq!(live.hand_holding(rifle().object), Some(Hand::Left));
    assert_eq!(live.pending, None, "the expectation is used up");
    assert_eq!(
        live.hand_moves(&tick(&slots, Some(rifle()), None)),
        None,
        "it stays where it landed"
    );
}

#[test]
fn the_game_s_own_draw_lands_in_the_dominant_hand() {
    let slots = [rifle()];
    let mut live = started(&slots);
    let arrival = live.hand_moves(&tick(&slots, Some(rifle()), None));
    assert_eq!(
        arrival,
        Some(Arrival {
            hand: Hand::Right,
            expect: None
        })
    );
}

#[test]
fn a_weapon_the_game_puts_away_leaves_its_hand_before_the_next_lands() {
    let slots = [rifle(), pistol()];
    let mut live = started(&slots);
    live.hand_moves(&tick(&slots, Some(rifle()), None));
    assert_eq!(live.hand_holding(rifle().object), Some(Hand::Right));
    live.hand_moves(&tick(&slots, Some(pistol()), None));
    assert_eq!(live.hand_holding(pistol().object), Some(Hand::Right));
    let docked = live
        .table
        .items()
        .find(|item| item.binding == Binding::Live(rifle().object))
        .map(|item| item.place);
    assert!(
        matches!(docked, Some(Place::Socket(socket)) if socket.is_native(WeaponSlot::E_SLOT_TWO_HANDED)),
        "the rifle docks on its own socket, not {docked:?}"
    );
}

#[test]
fn an_armed_weapon_stays_in_its_hand_beside_the_wielded_one() {
    let slots = [rifle(), pistol()];
    let mut live = started(&slots);
    live.hand_moves(&tick(&slots, Some(pistol()), None));
    draw(&mut live, Hand::Left, rifle());
    live.hand_moves(&tick(&slots, Some(pistol()), Some((Hand::Left, rifle()))));
    assert_eq!(live.hand_holding(pistol().object), Some(Hand::Right));
    assert_eq!(
        live.hand_holding(rifle().object),
        Some(Hand::Left),
        "an armed weapon is not taken for a departure"
    );
    live.hand_moves(&tick(&slots, Some(pistol()), None));
    assert_ne!(
        live.hand_holding(rifle().object),
        Some(Hand::Left),
        "a weapon neither wielded nor armed, back in its slot, docks"
    );
}

#[test]
fn a_weapon_the_game_wields_waits_while_its_hand_is_full() {
    let slots = [rifle(), pistol()];
    let mut live = started(&slots);
    live.hand_moves(&tick(&slots, Some(pistol()), None));
    draw(&mut live, Hand::Left, rifle());
    live.pending = Some(Pending::new(Hand::Left, Expect::Draw, Some(object(9))));
    let refusals = live.refusals;
    assert_eq!(
        live.land(Held {
            object: object(9),
            weapon: rifle().weapon
        }),
        None
    );
    assert_eq!(live.refusals, refusals, "nothing is refused");
    assert!(live.pending.is_some(), "the expectation is kept");
}
