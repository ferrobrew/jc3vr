use std::num::NonZeroUsize;

use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponClass};

use super::*;
use crate::controllers::{
    frame::Hand,
    loadout::{
        BodyLayout, ObjectAddress, Place, WeaponType,
        binding::{Awaiting, Binding},
        item::{ItemId, ItemKind},
        table::{Docked, Item, Loadout},
    },
    weapons::inventory::{
        carry::CarryRefusal,
        restore::adoption::{Adapter, Outcome, transfer},
    },
};

struct Fake {
    table: Loadout,
    physical: Vec<(ObjectAddress, Socket)>,
    refuse: bool,
}

impl Adapter for Fake {
    fn admit(&mut self, restored: Restored) -> Result<RestoreAdmission, RestoreRefusal> {
        self.table.admit_restore(restored)
    }
    fn carry(&mut self, admission: RestoreAdmission) -> Result<(), CarryRefusal> {
        if self.refuse {
            return Err(CarryRefusal::NotDone);
        }
        self.physical
            .push((admission.restored.object, admission.restored.socket));
        Ok(())
    }
    fn commit(&mut self, admission: RestoreAdmission) -> Result<Socket, RestoreRefusal> {
        self.table.commit_restore(admission)?;
        Ok(admission.restored.socket)
    }
    fn abandon(&mut self) {
        self.physical.pop();
    }
}

fn object(n: usize) -> ObjectAddress {
    ObjectAddress::new(NonZeroUsize::new(n).unwrap())
}

fn pistol() -> WeaponType {
    WeaponType::new(
        1,
        WeaponSlot::E_SLOT_DUAL_WIELD,
        WeaponClass::E_WEAPON_CLASS_PISTOL as i32,
    )
    .unwrap()
}

fn fixture(spare: bool) -> (Fake, Restored, ItemId) {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let socket = Socket::Hip(Hand::Left);
    let mut expected = None;
    for (index, place) in Socket::ALL.into_iter().enumerate() {
        if spare && place == Socket::Chest {
            continue;
        }
        let id = table.allocate();
        if place == socket {
            expected = Some(id);
        }
        table.sockets[place.index()] = Some(
            Docked::new(
                Item {
                    id,
                    kind: ItemKind::Weapon(pistol()),
                    binding: if place == socket {
                        Binding::Awaiting(Awaiting::Respawn)
                    } else {
                        Binding::Live(object(index + 10))
                    },
                },
                place,
            )
            .unwrap(),
        );
    }
    (
        Fake {
            table,
            physical: Vec::new(),
            refuse: false,
        },
        Restored {
            object: object(100),
            weapon: pistol(),
            socket,
        },
        expected.unwrap(),
    )
}

#[test]
fn full_reservations_restore_the_awaited_identity_on_its_original_socket() {
    for spare in [false, true] {
        let (mut adapter, restored, id) = fixture(spare);
        assert_eq!(
            transfer(&mut adapter, restored),
            Outcome::Restored(restored.socket)
        );
        assert_eq!(adapter.physical, vec![(restored.object, restored.socket)]);
        let item = adapter.table.items().find(|item| item.id == id).unwrap();
        assert_eq!(item.binding, Binding::Live(restored.object));
        assert_eq!(item.place, Place::Socket(restored.socket));
        if spare {
            assert!(adapter.table.sockets[Socket::Chest.index()].is_none());
        }
    }
}

#[test]
fn native_carry_refusal_changes_neither_table_nor_physical_destination() {
    let (mut adapter, restored, _) = fixture(false);
    adapter.refuse = true;
    let before = adapter.table.snapshot();
    assert_eq!(
        transfer(&mut adapter, restored),
        Outcome::CarryRefused(CarryRefusal::NotDone)
    );
    assert_eq!(adapter.table.snapshot(), before);
    assert!(adapter.physical.is_empty());
}

#[test]
fn new_weapon_without_free_socket_refuses_before_native_carry() {
    let (mut adapter, mut restored, _) = fixture(false);
    restored.weapon = WeaponType::new(
        2,
        WeaponSlot::E_SLOT_DUAL_WIELD,
        WeaponClass::E_WEAPON_CLASS_PISTOL as i32,
    )
    .unwrap();
    let before = adapter.table.snapshot();
    assert_eq!(
        transfer(&mut adapter, restored),
        Outcome::Refused(RestoreRefusal::NoSocket)
    );
    assert_eq!(adapter.table.snapshot(), before);
    assert!(adapter.physical.is_empty());
}
