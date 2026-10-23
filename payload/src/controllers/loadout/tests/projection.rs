//! The game's slots as a projection of the table.

use jc3gi::character::inventory::WeaponSlot;

use super::fixtures::*;
use crate::controllers::{
    frame::Hand,
    loadout::{
        item::{ItemId, ItemKind},
        projection::{BodyPlace, SlotContent, project, project_store},
        socket::Socket,
        table::Loadout,
        transition::Release,
    },
};

fn content(table: &Loadout, slot: WeaponSlot) -> SlotContent {
    project(table)
        .slots
        .into_iter()
        .find(|fill| fill.slot == slot)
        .map_or(SlotContent::Empty, |fill| fill.content)
}

fn holds(table: &Loadout, slot: WeaponSlot) -> Option<ItemId> {
    match content(table, slot) {
        SlotContent::Weapon { item, .. } => Some(item),
        SlotContent::Empty | SlotContent::Game => None,
    }
}

#[test]
fn the_primary_hand_s_weapon_is_in_its_slot_and_selected() {
    let mut t = table();
    let left = take(&mut t, Hand::Left, 1, weapon(a_rifle()));
    let right = take(&mut t, Hand::Right, 2, weapon(a_rifle()));
    let projection = project(&t);
    // Both are rifles of one slot: the primary (dominant) hand's goes in.
    assert_eq!(
        projection.slot_of(right),
        Some(WeaponSlot::E_SLOT_TWO_HANDED)
    );
    assert_eq!(projection.slot_of(left), None);
    assert_eq!(projection.selected, Some(WeaponSlot::E_SLOT_TWO_HANDED));
    t.let_go(Hand::Right, Release::Dropped).unwrap();
    let projection = project(&t);
    assert_eq!(
        projection.slot_of(left),
        Some(WeaponSlot::E_SLOT_TWO_HANDED)
    );
    assert_eq!(
        projection.slot_of(right),
        None,
        "a weapon in the world is never projected"
    );
    check_invariants(&t);
}

#[test]
fn the_other_hand_s_weapon_goes_before_a_socket_s() {
    let mut t = table();
    let on_hip = docked(&mut t, Socket::Hip(Hand::Right), 1, a_pistol());
    let held = take(&mut t, Hand::Left, 2, weapon(a_pistol()));
    take(&mut t, Hand::Right, 3, weapon(a_rifle()));
    assert_eq!(holds(&t, WeaponSlot::E_SLOT_DUAL_WIELD), Some(held));
    t.let_go(Hand::Left, Release::Dropped).unwrap();
    assert_eq!(holds(&t, WeaponSlot::E_SLOT_DUAL_WIELD), Some(on_hip));
}

#[test]
fn the_socket_the_game_docks_a_slot_on_goes_first() {
    let mut t = table();
    let outer = docked(&mut t, Socket::BackOuter, 1, a_rifle());
    let native = docked(&mut t, Socket::Shoulder(Hand::Right), 2, a_rifle());
    assert!(outer < native);
    assert_eq!(holds(&t, WeaponSlot::E_SLOT_TWO_HANDED), Some(native));
}

#[test]
fn a_held_heavy_weapon_is_never_in_the_heavy_slot() {
    let mut t = table();
    let launcher = take(&mut t, Hand::Right, 1, weapon(a_launcher()));
    let projection = project(&t);
    assert_eq!(projection.slot_of(launcher), None);
    assert_eq!(projection.selected, None);
}

#[test]
fn a_heavy_weapon_on_the_back_stays_out_of_the_heavy_slot_and_in_the_sidecar() {
    let mut t = table();
    let launcher = docked(&mut t, Socket::BackOuter, 1, a_launcher());
    assert_eq!(holds(&t, WeaponSlot::E_SLOT_HEAVY), None);
    let store = project_store(&t);
    let entry = store.sidecar.iter().find(|e| e.item == launcher);
    assert_eq!(
        entry.map(|e| (e.place, e.slot)),
        Some((BodyPlace::Socket(Socket::BackOuter), None))
    );
}

#[test]
fn the_consumables_slots_stay_the_game_s() {
    let mut t = table();
    take(&mut t, Hand::Right, 1, ItemKind::Grenade);
    take(&mut t, Hand::Left, 2, ItemKind::Charge);
    for slot in [
        WeaponSlot::E_SLOT_HAND_GRENADE,
        WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE,
        WeaponSlot::E_SLOT_FOW,
        WeaponSlot::E_SLOT_BEACON,
    ] {
        assert_eq!(content(&t, slot), SlotContent::Game);
    }
    assert_eq!(project(&t).selected, None);
}

#[test]
fn the_sidecar_records_every_weapon_on_the_body_and_its_slot() {
    let mut t = table();
    let held = take(&mut t, Hand::Left, 1, weapon(a_rifle()));
    let extra = docked(&mut t, Socket::BackOuter, 2, a_rifle());
    let pistol = docked(&mut t, Socket::Hip(Hand::Left), 3, a_pistol());
    take(&mut t, Hand::Right, 4, ItemKind::Prop);
    let store = project_store(&t);
    let recorded: Vec<(ItemId, BodyPlace, Option<WeaponSlot>)> = store
        .sidecar
        .iter()
        .map(|entry| (entry.item, entry.place, entry.slot))
        .collect();
    assert_eq!(
        recorded,
        vec![
            (
                held,
                BodyPlace::Hand(Hand::Left),
                Some(WeaponSlot::E_SLOT_TWO_HANDED)
            ),
            (
                pistol,
                BodyPlace::Socket(Socket::Hip(Hand::Left)),
                Some(WeaponSlot::E_SLOT_DUAL_WIELD)
            ),
            (extra, BodyPlace::Socket(Socket::BackOuter), None),
        ]
    );
}
