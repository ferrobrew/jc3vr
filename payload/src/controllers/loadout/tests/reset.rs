//! The spawn reset's re-keying, and the checkpoint and mission snapshots.

use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};
use test_case::test_case;

use super::fixtures::*;
use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Awaiting, Binding},
        item::{ItemId, ItemKind, WeaponType},
        projection::{SlotContent, project},
        reset::{CacheKind, ResetKind, SlotObject},
        socket::{BodyLayout, Socket},
        table::{Departure, ItemView, Loadout, Place},
        transition::{Cause, Change, Release, Removal, RemovalWhy, Restored, TransitionError},
    },
};

fn slot(n: usize, weapon: WeaponType) -> SlotObject {
    SlotObject {
        object: object(n),
        weapon,
    }
}

fn binding(table: &Loadout, id: ItemId) -> Option<Binding> {
    view(table, id).map(|item| item.binding)
}

/// A rifle in the right hand, a pistol on the right hip, and a second rifle beyond the slots on
/// the back.
fn armed() -> (Loadout, [ItemId; 3]) {
    let mut t = table();
    let rifle = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    let pistol = docked(&mut t, Socket::Hip(Hand::Right), 2, a_pistol());
    let extra = docked(&mut t, Socket::BackOuter, 3, a_rifle());
    (t, [rifle, pistol, extra])
}

#[test]
fn the_reset_unbinds_only_what_the_game_s_slots_hold() {
    let (mut t, [rifle, pistol, extra]) = armed();
    let launcher = take(&mut t, Hand::Left, 4, weapon(a_launcher()));
    let began = t.begin_reset();
    assert!(began.iter().all(|tr| tr.cause == Cause::ResetBegan));
    assert_eq!(
        binding(&t, rifle),
        Some(Binding::Awaiting(Awaiting::Slot(
            WeaponSlot::E_SLOT_TWO_HANDED
        )))
    );
    assert_eq!(
        binding(&t, pistol),
        Some(Binding::Awaiting(Awaiting::Slot(
            WeaponSlot::E_SLOT_DUAL_WIELD
        )))
    );
    // The rifle beyond the slots and the held heavy weapon, never in a slot in play, keep theirs.
    assert_eq!(binding(&t, extra), Some(Binding::Live(object(3))));
    assert_eq!(binding(&t, launcher), Some(Binding::Live(object(4))));
    assert!(t.begin_reset().is_empty(), "a second begin changes nothing");
    check_invariants(&t);
}

#[test_case(ResetKind::Travel => Some(Place::Hand(Hand::Right)); "a travel keeps the hands")]
#[test_case(ResetKind::Respawn => Some(Place::Socket(Socket::Shoulder(Hand::Right))); "a respawn empties them onto the sockets")]
fn the_reset_re_keys_the_slots_weapons_and_keeps_their_identity(kind: ResetKind) -> Option<Place> {
    let (mut t, [rifle, pistol, extra]) = armed();
    t.begin_reset();
    let done = t
        .complete_reset(kind, &[slot(11, a_rifle()), slot(12, a_pistol())])
        .unwrap();
    assert!(done.iter().all(|tr| tr.cause == Cause::Reset(kind)));
    assert_eq!(binding(&t, rifle), Some(Binding::Live(object(11))));
    assert_eq!(binding(&t, pistol), Some(Binding::Live(object(12))));
    assert_eq!(binding(&t, extra), Some(Binding::Live(object(3))));
    assert_eq!(
        place(&t, pistol),
        Some(Place::Socket(Socket::Hip(Hand::Right)))
    );
    assert_eq!(t.items().count(), 3, "no item made, none lost");
    check_invariants(&t);
    place(&t, rifle)
}

#[test]
fn a_respawn_drops_the_held_props_and_the_world_s_weapons() {
    let mut t = table();
    let dropped = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    t.let_go(Hand::Right, Release::Dropped).unwrap();
    let prop = take(&mut t, Hand::Left, 2, ItemKind::Prop);
    t.begin_reset();
    t.complete_reset(ResetKind::Respawn, &[]).unwrap();
    assert_eq!(view(&t, dropped), None);
    assert_eq!(view(&t, prop), None);
}

#[test]
fn a_slot_the_reset_emptied_or_changed_takes_its_weapon_away() {
    let (mut t, [rifle, pistol, _]) = armed();
    t.begin_reset();
    // The game gave the two-handed slot another type and emptied the pistols.
    let other = weapon_type(
        0x999,
        WeaponSlot::E_SLOT_TWO_HANDED,
        WeaponClass::E_WEAPON_CLASS_SHOTGUN,
    );
    let done = t
        .complete_reset(ResetKind::Travel, &[slot(11, other)])
        .unwrap();
    assert_eq!(view(&t, rifle), None);
    assert_eq!(view(&t, pistol), None);
    let new = slot_item(&t, WeaponSlot::E_SLOT_TWO_HANDED);
    assert!(new.is_some_and(|item| item.binding == Binding::Live(object(11))));
    assert_eq!(
        done.iter()
            .filter(|tr| matches!(tr.change, Change::Left { .. }))
            .count(),
        2
    );
    check_invariants(&t);
}

#[test]
fn a_load_starts_the_table_again_from_the_slots() {
    let (mut t, ids) = armed();
    t.begin_reset();
    t.complete_reset(ResetKind::Load, &[slot(11, a_rifle())])
        .unwrap();
    assert!(ids.iter().all(|id| view(&t, *id).is_none()));
    let items: Vec<ItemView> = t.items().collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].place, Place::Socket(Socket::Shoulder(Hand::Right)));
}

#[test]
fn a_report_naming_a_slot_twice_is_refused_whole() {
    let (mut t, _) = armed();
    t.begin_reset();
    let before = t.snapshot();
    assert_eq!(
        t.complete_reset(
            ResetKind::Travel,
            &[slot(11, a_rifle()), slot(12, a_rifle())]
        ),
        Err(TransitionError::DuplicateSlotObject(object(12)))
    );
    assert_eq!(t.snapshot(), before);
}

#[test]
fn a_checkpoint_restore_returns_the_table_to_its_snapshot() {
    let (mut t, [rifle, pistol, extra]) = armed();
    t.cache(CacheKind::Checkpoint);
    // Since the checkpoint: the rifle swaps places with the extra one, the pistol is dropped and
    // gone, and a sniper rifle is picked up.
    t.dock(Hand::Right, Socket::Shoulder(Hand::Left)).unwrap();
    t.draw(Socket::BackOuter, Hand::Right).unwrap();
    t.draw(Socket::Shoulder(Hand::Left), Hand::Left).unwrap();
    t.dock(Hand::Left, Socket::BackOuter).unwrap();
    t.draw(Socket::Hip(Hand::Right), Hand::Left).unwrap();
    t.let_go(Hand::Left, Release::Dropped).unwrap();
    t.remove(Removal::Object(object(2)), RemovalWhy::Gone)
        .unwrap();
    let sniper = take(&mut t, Hand::Left, 5, weapon(a_sniper()));
    let restored = t
        .restore(
            CacheKind::Checkpoint,
            &[slot(1, a_rifle()), slot(12, a_pistol())],
        )
        .unwrap();
    assert!(
        restored
            .iter()
            .all(|tr| tr.cause == Cause::Restore(CacheKind::Checkpoint))
    );
    assert_eq!(view(&t, sniper), None, "gained since, so gone");
    // The hands come back empty: the rifle held at the checkpoint goes onto its shoulder.
    assert_eq!(
        place(&t, rifle),
        Some(Place::Socket(Socket::Shoulder(Hand::Right)))
    );
    assert_eq!(place(&t, extra), Some(Place::Socket(Socket::BackOuter)));
    // The pistol's object was gone; the restored slot's new pistol is it again.
    assert_eq!(
        place(&t, pistol),
        Some(Place::Socket(Socket::Hip(Hand::Right)))
    );
    assert_eq!(binding(&t, pistol), Some(Binding::Live(object(12))));
    check_invariants(&t);
}

#[test]
fn a_restored_weapon_whose_object_is_gone_waits_for_its_spawn() {
    let (mut t, [_, _, extra]) = armed();
    t.cache(CacheKind::Checkpoint);
    t.remove(Removal::Item(extra), RemovalWhy::Gone).unwrap();
    t.restore(CacheKind::Checkpoint, &[]).unwrap();
    assert_eq!(
        binding(&t, extra),
        Some(Binding::Awaiting(Awaiting::Respawn))
    );
    assert_eq!(place(&t, extra), Some(Place::Socket(Socket::BackOuter)));
    t.spawned(extra, object(20)).unwrap();
    assert_eq!(binding(&t, extra), Some(Binding::Live(object(20))));
}

#[test]
fn a_weapon_the_mod_respawns_binds_the_restored_item_waiting_for_it() {
    let (mut t, [_, _, extra]) = armed();
    t.cache(CacheKind::Checkpoint);
    t.remove(Removal::Item(extra), RemovalWhy::Gone).unwrap();
    t.restore(CacheKind::Checkpoint, &[]).unwrap();
    let before = t.items().count();
    let out = t
        .restored(Restored {
            object: object(21),
            weapon: a_rifle(),
            socket: Socket::Shoulder(Hand::Left),
        })
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].item, extra, "the waiting item keeps its identity");
    assert_eq!(binding(&t, extra), Some(Binding::Live(object(21))));
    assert_eq!(place(&t, extra), Some(Place::Socket(Socket::BackOuter)));
    assert_eq!(t.items().count(), before, "nothing is duplicated");
}

#[test]
fn a_mission_s_give_back_brings_the_snapshot_back_beside_what_was_taken_since() {
    let (mut t, [rifle, pistol, extra]) = armed();
    t.cache(CacheKind::Mission);
    t.remove(Removal::AllWeapons, RemovalWhy::Mission).unwrap();
    assert_eq!(t.items().count(), 0);
    let found = take(&mut t, Hand::Right, 7, weapon(an_smg()));
    t.restore(
        CacheKind::Mission,
        &[slot(11, a_rifle()), slot(12, a_pistol())],
    )
    .unwrap();
    assert_eq!(place(&t, found), Some(Place::Hand(Hand::Right)), "kept");
    assert_eq!(
        place(&t, rifle),
        Some(Place::Socket(Socket::Shoulder(Hand::Right)))
    );
    assert_eq!(binding(&t, rifle), Some(Binding::Live(object(11))));
    assert_eq!(binding(&t, pistol), Some(Binding::Live(object(12))));
    assert_eq!(
        binding(&t, extra),
        Some(Binding::Awaiting(Awaiting::Respawn))
    );
    check_invariants(&t);
}

#[test]
fn a_restore_without_a_snapshot_is_refused() {
    let mut t = table();
    assert_eq!(
        t.restore(CacheKind::Challenge, &[]),
        Err(TransitionError::NoCache(CacheKind::Challenge))
    );
    t.cache(CacheKind::Challenge);
    assert_eq!(
        t.caches_held().collect::<Vec<_>>(),
        vec![CacheKind::Challenge]
    );
}

#[test]
fn a_give_back_uses_its_snapshot_up_and_a_checkpoint_keeps_its_own() {
    let (mut t, _) = armed();
    t.cache(CacheKind::Checkpoint);
    t.cache(CacheKind::Challenge);
    t.restore(CacheKind::Challenge, &[]).unwrap();
    t.restore(CacheKind::Checkpoint, &[]).unwrap();
    assert_eq!(
        t.caches_held().collect::<Vec<_>>(),
        vec![CacheKind::Checkpoint],
        "a second death at the checkpoint restores it again"
    );
    assert_eq!(
        t.restore(CacheKind::Challenge, &[]),
        Err(TransitionError::NoCache(CacheKind::Challenge))
    );
}

#[test]
fn a_returned_weapon_lies_at_the_feet_after_a_restore_with_the_back_full() {
    let mut t = Loadout::new(BodyLayout::new(Hand::Right, 1, 0, false));
    let rifle = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    t.cache(CacheKind::Checkpoint);
    docked(&mut t, Socket::Shoulder(Hand::Right), 2, a_rifle());
    // The rifle docked since was gained after the snapshot, so it goes and the shoulder is free.
    t.restore(CacheKind::Checkpoint, &[]).unwrap();
    assert_eq!(
        place(&t, rifle),
        Some(Place::Socket(Socket::Shoulder(Hand::Right)))
    );
    let mut t = Loadout::new(BodyLayout::new(Hand::Right, 0, 0, false));
    let rifle = take(&mut t, Hand::Right, 1, weapon(a_rifle()));
    t.cache(CacheKind::Checkpoint);
    t.restore(CacheKind::Checkpoint, &[]).unwrap();
    assert_eq!(place(&t, rifle), Some(Place::World(Departure::Returned)));
}

/// The item the play projection puts in `slot`.
fn slot_item(table: &Loadout, slot: WeaponSlot) -> Option<ItemView> {
    let projection = project(table);
    let fill = projection.slots.into_iter().find(|f| f.slot == slot)?;
    match fill.content {
        SlotContent::Weapon { item, .. } => view(table, item),
        SlotContent::Empty | SlotContent::Game => None,
    }
}
