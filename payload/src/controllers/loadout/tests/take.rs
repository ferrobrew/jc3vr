//! Where a weapon taken from the world goes: the grabbing hand through the table, the game's
//! pickup where the table cannot hold it, and the refusals.

use jc3gi::character::inventory::WeaponSlot;
use pretty_assertions::assert_eq;

use super::fixtures::*;
use crate::controllers::{
    frame::Hand,
    loadout::{
        projection::{ProjectionMode, SlotContent, project},
        socket::Socket,
        table::Place,
        take::{GameTake, TakeOffer, TakeRefusal, TakeRoute, TakeRules, route},
        transition::{Take, TakeSource},
    },
};

const BOTH: TakeRules = TakeRules {
    either_hand: true,
    both_hands: true,
};
const ONE: TakeRules = TakeRules {
    either_hand: true,
    both_hands: false,
};

fn offer(hand: Option<Hand>, weapon: crate::controllers::loadout::WeaponType) -> TakeOffer {
    TakeOffer {
        hand,
        weapon,
        swaps: false,
    }
}

/// The failing case the takes are built for: with a rifle in the right hand and the slot's pistol
/// on its hip, the left hand takes a pistol out of a character's hands. The pistol lands in the
/// left hand as its own item, the rifle stays in the right, and the hip's pistol stays on the hip.
#[test]
fn a_pistol_stolen_with_the_left_hand_leaves_the_right_hand_s_rifle_alone() {
    let mut t = table();
    let hip = docked(&mut t, Socket::Hip(Hand::Right), 1, a_pistol());
    let rifle = take(&mut t, Hand::Right, 2, weapon(a_rifle()));
    let s = t.snapshot();
    let routed = route(Some(&s), offer(Some(Hand::Left), a_pistol()), BOTH);
    assert_eq!(routed, Ok(TakeRoute::Table { hand: Hand::Left }));
    let transitions = t
        .take(Take {
            hand: Hand::Left,
            object: object(3),
            kind: weapon(a_pistol()),
            source: TakeSource::Character,
        })
        .unwrap();
    let stolen = transitions[0].item;
    assert_ne!(stolen, hip, "the stolen pistol is an item of its own");
    assert_eq!(place(&t, stolen), Some(Place::Hand(Hand::Left)));
    assert_eq!(place(&t, rifle), Some(Place::Hand(Hand::Right)));
    assert_eq!(
        place(&t, hip),
        Some(Place::Socket(Socket::Hip(Hand::Right)))
    );
    assert_eq!(
        t.primary(),
        Some(Hand::Right),
        "the rifle stays the wielded one"
    );
    let projection = project(&t, ProjectionMode::Play);
    let in_slot = |slot| {
        projection
            .slots
            .iter()
            .find(|fill| fill.slot == slot)
            .map(|fill| fill.content)
    };
    assert!(matches!(
        in_slot(WeaponSlot::E_SLOT_TWO_HANDED),
        Some(SlotContent::Weapon { item, .. }) if item == rifle
    ));
    assert!(
        matches!(
            in_slot(WeaponSlot::E_SLOT_DUAL_WIELD),
            Some(SlotContent::Weapon { item, .. }) if item == stolen
        ),
        "the hand's pistol takes the slot beside the hip's, which stays on the hip"
    );
}

#[test]
fn a_take_lands_in_the_grabbing_hand_whatever_the_other_holds() {
    let mut t = table();
    take(&mut t, Hand::Left, 1, weapon(a_rifle()));
    let s = t.snapshot();
    assert_eq!(
        route(Some(&s), offer(Some(Hand::Right), a_rifle()), BOTH),
        Ok(TakeRoute::Table { hand: Hand::Right })
    );
    assert_eq!(
        route(Some(&s), offer(Some(Hand::Left), a_rifle()), BOTH),
        Err(TakeRefusal::HandFull(Hand::Left))
    );
}

#[test]
fn the_use_item_pickup_takes_a_free_hand_the_dominant_first() {
    let mut t = table();
    let s = t.snapshot();
    assert_eq!(
        route(Some(&s), offer(None, a_rifle()), BOTH),
        Ok(TakeRoute::Table { hand: Hand::Right })
    );
    take(&mut t, Hand::Right, 1, weapon(a_pistol()));
    let s = t.snapshot();
    assert_eq!(
        route(Some(&s), offer(None, a_rifle()), BOTH),
        Ok(TakeRoute::Table { hand: Hand::Left })
    );
    take(&mut t, Hand::Left, 2, weapon(a_pistol()));
    let s = t.snapshot();
    assert_eq!(
        route(Some(&s), offer(None, a_rifle()), BOTH),
        Err(TakeRefusal::BothHandsFull)
    );
}

#[test]
fn the_game_takes_what_the_table_cannot_hold() {
    let mut t = table();
    assert_eq!(
        route(None, offer(Some(Hand::Left), a_rifle()), BOTH),
        Ok(TakeRoute::Game(GameTake::TableStopped))
    );
    let s = t.snapshot();
    assert_eq!(
        route(
            Some(&s),
            offer(Some(Hand::Left), a_rifle()),
            TakeRules {
                either_hand: false,
                both_hands: true
            }
        ),
        Ok(TakeRoute::Game(GameTake::GameLayout))
    );
    assert_eq!(
        route(Some(&s), offer(Some(Hand::Left), a_launcher()), BOTH),
        Ok(TakeRoute::Game(GameTake::Heavy))
    );
    assert_eq!(
        route(
            Some(&s),
            TakeOffer {
                swaps: true,
                ..offer(Some(Hand::Left), a_rifle())
            },
            BOTH
        ),
        Ok(TakeRoute::Game(GameTake::Swaps))
    );
    take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    let s = t.snapshot();
    assert_eq!(
        route(Some(&s), offer(Some(Hand::Left), a_pistol()), ONE),
        Ok(TakeRoute::Game(GameTake::OneWeapon)),
        "one weapon at a time swaps through the game's pickup"
    );
}
