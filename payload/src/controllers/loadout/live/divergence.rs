//! Where the live table and the old weapon handling disagree. Pure.
//!
//! While the table is dual-written beside the old handling, nothing reads it, so the only check
//! that the dual-write is complete is to compare the two: for every weapon the old handling knows,
//! the table should have it in the matching place, and the table should have nothing the old
//! handling does not know. Each disagreement is a [`Divergence`], shown by the readouts, so a move
//! the dual-write misses shows before anything is made to read the table.

use std::fmt;

use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Binding, ObjectAddress},
        socket::Socket,
        table::{LoadoutSnapshot, Place},
    },
    weapons::holster::slot_label,
};

/// Where the old handling has a weapon.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum OldPlace {
    /// In its slot, holstered on the game's dock bone.
    Slot(WeaponSlot),
    Hand(Hand),
    /// In the carry set, on this socket.
    Socket(Socket),
    /// Lying in the world.
    World,
    /// Not the player's: with another character, or stashed by a mission.
    Elsewhere,
}

impl fmt::Display for OldPlace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OldPlace::Slot(slot) => write!(f, "in the {} slot", slot_label(*slot)),
            OldPlace::Hand(hand) => write!(f, "in the {} hand", hand.label()),
            OldPlace::Socket(socket) => write!(f, "carried on {socket}"),
            OldPlace::World => f.write_str("in the world"),
            OldPlace::Elsewhere => f.write_str("not the player's"),
        }
    }
}

/// One weapon the table and the old handling disagree about.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Divergence {
    pub object: ObjectAddress,
    /// Where the old handling has it, or `None` for a weapon it does not know.
    pub(crate) old: Option<OldPlace>,
    /// Where the table has it, or `None` for a weapon it does not have.
    pub(crate) table: Option<Place>,
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "object {}: ", self.object)?;
        match self.old {
            Some(old) => write!(f, "the old handling has it {old}")?,
            None => f.write_str("the old handling does not know it")?,
        }
        match self.table {
            Some(place) => write!(f, ", the table in {place}"),
            None => f.write_str(", the table does not have it"),
        }
    }
}

/// Every disagreement between `old`, where the old handling has each weapon it knows, and
/// `snapshot`. A slot's weapon matches a socket the game docks that slot on; an item waiting for an
/// object is not compared.
pub(crate) fn diverging(
    old: &[(ObjectAddress, OldPlace)],
    snapshot: &LoadoutSnapshot,
) -> Vec<Divergence> {
    let table_place = |object: ObjectAddress| {
        snapshot
            .items
            .iter()
            .find(|item| item.binding == Binding::Live(object))
            .map(|item| item.place)
    };
    let agree = |old: OldPlace, table: Option<Place>| match (old, table) {
        (OldPlace::Slot(slot), Some(Place::Socket(socket))) => socket.is_native(slot),
        (OldPlace::Hand(old), Some(Place::Hand(table))) => old == table,
        (OldPlace::Socket(old), Some(Place::Socket(table))) => old == table,
        (OldPlace::World, Some(Place::World(_))) | (OldPlace::Elsewhere, None) => true,
        _ => false,
    };
    let known = old.iter().filter_map(|&(object, old)| {
        let table = table_place(object);
        (!agree(old, table)).then_some(Divergence {
            object,
            old: Some(old),
            table,
        })
    });
    let unknown = snapshot.items.iter().filter_map(|item| {
        let object = item.binding.address()?;
        (!old.iter().any(|&(o, _)| o == object)).then_some(Divergence {
            object,
            old: None,
            table: Some(item.place),
        })
    });
    known.chain(unknown).collect()
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use jc3gi::equipment::weapon::WeaponClass;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::controllers::loadout::{
        event::Event,
        item::{ItemKind, WeaponType},
        socket::BodyLayout,
        table::Loadout,
        transition::{Release, Take, TakeSource},
    };

    fn object(n: usize) -> ObjectAddress {
        ObjectAddress::new(NonZeroUsize::MIN.saturating_add(n * 0x10))
    }

    fn rifle() -> ItemKind {
        ItemKind::Weapon(
            WeaponType::new(
                0x300,
                WeaponSlot::E_SLOT_TWO_HANDED,
                WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE as i32,
            )
            .unwrap(),
        )
    }

    fn take(table: &mut Loadout, hand: Hand, n: usize) {
        table
            .fold(Event::Take(Take {
                hand,
                object: object(n),
                kind: rifle(),
                source: TakeSource::World,
            }))
            .unwrap();
    }

    #[test]
    fn matching_places_agree() {
        let mut t = Loadout::new(BodyLayout::full(Hand::Right));
        take(&mut t, Hand::Right, 1);
        take(&mut t, Hand::Left, 2);
        let native = Socket::ALL
            .into_iter()
            .find(|s| s.is_native(WeaponSlot::E_SLOT_TWO_HANDED))
            .unwrap();
        t.fold(Event::Dock {
            hand: Hand::Left,
            socket: native,
        })
        .unwrap();
        take(&mut t, Hand::Left, 3);
        t.fold(Event::LetGo {
            hand: Hand::Left,
            release: Release::Dropped,
        })
        .unwrap();
        let old = [
            (object(1), OldPlace::Hand(Hand::Right)),
            (object(2), OldPlace::Slot(WeaponSlot::E_SLOT_TWO_HANDED)),
            (object(3), OldPlace::World),
            (object(4), OldPlace::Elsewhere),
        ];
        assert_eq!(diverging(&old, &t.snapshot()), Vec::new());
    }

    #[test]
    fn each_disagreement_is_reported_once() {
        let mut t = Loadout::new(BodyLayout::full(Hand::Right));
        take(&mut t, Hand::Right, 1);
        take(&mut t, Hand::Left, 2);
        let old = [
            (object(1), OldPlace::Hand(Hand::Left)),
            (object(3), OldPlace::World),
        ];
        let found = diverging(&old, &t.snapshot());
        assert_eq!(
            found,
            vec![
                Divergence {
                    object: object(1),
                    old: Some(OldPlace::Hand(Hand::Left)),
                    table: Some(Place::Hand(Hand::Right)),
                },
                Divergence {
                    object: object(3),
                    old: Some(OldPlace::World),
                    table: None,
                },
                Divergence {
                    object: object(2),
                    old: None,
                    table: Some(Place::Hand(Hand::Left)),
                },
            ]
        );
        assert_eq!(
            found[1].to_string(),
            format!(
                "object {}: the old handling has it in the world, the table does not have it",
                object(3)
            )
        );
    }
}
