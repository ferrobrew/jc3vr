//! The fixtures the loadout's tests share: made-up objects and weapons, a table, quick moves, and
//! the invariant check.

use std::num::NonZeroUsize;

use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};
use pretty_assertions::assert_eq;

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Binding, ObjectAddress},
        item::{ItemId, ItemKind, WeaponType},
        projection::{SLOTS, SlotContent, project},
        socket::{BodyLayout, Socket},
        table::{ItemView, Loadout, Place, primary},
        transition::{Change, Take, TakeSource, Transition},
    },
};

/// A made-up object at an address derived from `n`.
pub(super) fn object(n: usize) -> ObjectAddress {
    ObjectAddress::new(NonZeroUsize::MIN.saturating_add(n * 0x10))
}

pub(super) fn weapon_type(tag: u32, slot: WeaponSlot, class: WeaponClass) -> WeaponType {
    match WeaponType::new(tag, slot, class as i32) {
        Ok(weapon) => weapon,
        Err(error) => panic!("{error}"),
    }
}

pub(super) fn a_pistol() -> WeaponType {
    weapon_type(
        0x100,
        WeaponSlot::E_SLOT_DUAL_WIELD,
        WeaponClass::E_WEAPON_CLASS_PISTOL,
    )
}

pub(super) fn an_smg() -> WeaponType {
    weapon_type(
        0x200,
        WeaponSlot::E_SLOT_DUAL_WIELD,
        WeaponClass::E_WEAPON_CLASS_SMG,
    )
}

pub(super) fn a_rifle() -> WeaponType {
    weapon_type(
        0x300,
        WeaponSlot::E_SLOT_TWO_HANDED,
        WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE,
    )
}

pub(super) fn a_sniper() -> WeaponType {
    weapon_type(
        0x400,
        WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
        WeaponClass::E_WEAPON_CLASS_SNIPER,
    )
}

pub(super) fn a_launcher() -> WeaponType {
    weapon_type(
        0x500,
        WeaponSlot::E_SLOT_HEAVY,
        WeaponClass::E_WEAPON_CLASS_RPG,
    )
}

pub(super) const fn weapon(weapon: WeaponType) -> ItemKind {
    ItemKind::Weapon(weapon)
}

/// An empty table for a right-handed player with every socket.
pub(super) fn table() -> Loadout {
    Loadout::new(BodyLayout::full(Hand::Right))
}

/// Take a new object `n` of `kind` into `hand`, from wherever such an item is taken.
pub(super) fn take(table: &mut Loadout, hand: Hand, n: usize, kind: ItemKind) -> ItemId {
    let source = match kind {
        ItemKind::Weapon(_) | ItemKind::Prop => TakeSource::World,
        ItemKind::Grenade | ItemKind::Charge => TakeSource::Pouch,
    };
    let transitions = table
        .take(Take {
            hand,
            object: object(n),
            kind,
            source,
        })
        .unwrap_or_else(|error| panic!("{error}"));
    transitions[0].item
}

/// Take a weapon `n` into `hand` and dock it on `socket`.
pub(super) fn docked(table: &mut Loadout, socket: Socket, n: usize, kind: WeaponType) -> ItemId {
    let hand = [Hand::Right, Hand::Left]
        .into_iter()
        .find(|&h| table.held(h).is_none())
        .unwrap_or_else(|| panic!("a hand is free to dock with"));
    let id = take(table, hand, n, weapon(kind));
    table
        .dock(hand, socket)
        .unwrap_or_else(|error| panic!("{error}"));
    id
}

pub(super) fn view(table: &Loadout, id: ItemId) -> Option<ItemView> {
    table.items().find(|item| item.id == id)
}

pub(super) fn place(table: &Loadout, id: ItemId) -> Option<Place> {
    view(table, id).map(|item| item.place)
}

/// Every invariant the table keeps, checked from what the rest of the mod reads.
pub(super) fn check_invariants(table: &Loadout) {
    let items: Vec<ItemView> = table.items().collect();
    for (index, item) in items.iter().enumerate() {
        assert!(
            items[..index].iter().all(|other| other.id != item.id),
            "{} is in two places",
            item.id
        );
        if let Binding::Live(address) = item.binding {
            assert!(
                items[..index]
                    .iter()
                    .all(|other| other.binding != Binding::Live(address)),
                "two items are bound to object {address}"
            );
        }
        match item.place {
            Place::Socket(socket) => {
                let size = item.kind.weapon().map(|w| w.size());
                assert!(
                    size.is_some_and(|size| socket.fits(size)),
                    "{} ({}) is on {socket}, which does not take it",
                    item.id,
                    item.kind
                );
            }
            Place::World(_) => assert!(
                item.kind.weapon().is_some(),
                "{} lies in the world but is no weapon",
                item.id
            ),
            Place::Hand(_) => {}
        }
    }
    for hand in Hand::BOTH {
        let holding = items
            .iter()
            .filter(|item| item.place == Place::Hand(hand))
            .count();
        assert!(holding <= 1, "the {} hand holds {holding}", hand.label());
    }
    for socket in Socket::ALL {
        let holding = items
            .iter()
            .filter(|item| item.place == Place::Socket(socket))
            .count();
        assert!(holding <= 1, "{socket} holds {holding}");
    }
    let hands = Hand::BOTH.map(|hand| table.held(hand).map(|item| item.kind));
    assert_eq!(table.primary(), primary(hands, table.body().dominant()));
    check_projection(table, &items);
}

fn check_projection(table: &Loadout, items: &[ItemView]) {
    let projection = project(table);
    for (fill, slot) in projection.slots.iter().zip(SLOTS) {
        assert_eq!(fill.slot, slot);
        match fill.content {
            SlotContent::Weapon { item, weapon } => {
                assert!(
                    slot != WeaponSlot::E_SLOT_HEAVY,
                    "a heavy weapon is in the heavy slot"
                );
                assert_eq!(weapon.slot(), slot);
                let view = items.iter().find(|i| i.id == item);
                let view =
                    view.unwrap_or_else(|| panic!("{item} is projected but not in the table"));
                assert!(matches!(view.binding, Binding::Live(_)));
                assert!(
                    !matches!(view.place, Place::World(_)),
                    "a weapon in the world is projected"
                );
                let twice = projection
                    .slots
                    .iter()
                    .filter(
                        |f| matches!(f.content, SlotContent::Weapon { item: i, .. } if i == item),
                    )
                    .count();
                assert_eq!(twice, 1, "{item} is in more than one slot");
            }
            SlotContent::Empty => {}
            SlotContent::Game => assert!(matches!(
                slot,
                WeaponSlot::E_SLOT_HAND_GRENADE
                    | WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE
                    | WeaponSlot::E_SLOT_FOW
                    | WeaponSlot::E_SLOT_BEACON
            )),
        }
    }
    if let Some(selected) = projection.selected {
        let primary = table.primary().and_then(|hand| table.held(hand));
        let fill = projection.slots.iter().find(|f| f.slot == selected);
        assert!(
            matches!(
                (primary, fill.map(|f| f.content)),
                (Some(p), Some(SlotContent::Weapon { item, .. })) if p.id == item
            ),
            "the selected slot does not hold the primary hand's weapon"
        );
    }
}

/// The places the transitions say each item ended in, for checking against the table.
pub(super) fn ends(transitions: &[Transition]) -> Vec<(ItemId, Option<Place>)> {
    let mut out: Vec<(ItemId, Option<Place>)> = Vec::new();
    for transition in transitions {
        let end = match transition.change {
            Change::Entered { to } | Change::Moved { to, .. } => Some(to),
            Change::Left { .. } => None,
            Change::Bound { .. } | Change::Unbound { .. } => continue,
        };
        out.retain(|(id, _)| *id != transition.item);
        out.push((transition.item, end));
    }
    out
}
