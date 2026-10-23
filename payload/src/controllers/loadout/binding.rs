//! The one place an item meets the live game: the address of the game object it is bound to.
//!
//! The table keys every item by its own identity ([`crate::controllers::loadout::ItemId`]) and
//! keeps beside it the object the item is now, so that the spawn reset, which replaces each slot's
//! weapon with a new object of the same type, re-binds the identity rather than making a new item.
//! The loadout never reads through an address; the hooks and the reconcile that turn one into an
//! object do, and they alone call into the game.

use std::{fmt, num::NonZeroUsize};

use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::weapons::holster::slot_label;

/// A live game object's address: a weapon, a prop's rigid object, or a held grenade or charge.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectAddress(NonZeroUsize);

impl ObjectAddress {
    pub const fn new(address: NonZeroUsize) -> ObjectAddress {
        ObjectAddress(address)
    }
}

impl fmt::Display for ObjectAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

/// Which object an item is now.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Binding {
    /// A live object.
    Live(ObjectAddress),
    /// None yet: the item waits for the game's object of its type.
    Awaiting(Awaiting),
}

impl Binding {
    /// The live object's address, while bound to one.
    pub const fn address(self) -> Option<ObjectAddress> {
        match self {
            Binding::Live(address) => Some(address),
            Binding::Awaiting(_) => None,
        }
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Binding::Live(address) => write!(f, "object {address}"),
            Binding::Awaiting(Awaiting::Slot(slot)) => write!(
                f,
                "waiting for the spawn reset's new weapon in the {} slot",
                slot_label(*slot)
            ),
            Binding::Awaiting(Awaiting::Respawn) => f.write_str("waiting to be spawned again"),
        }
    }
}

/// What an unbound item waits for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Awaiting {
    /// The spawn reset's new object of the item's type in this slot.
    Slot(WeaponSlot),
    /// A spawn of the item's type: an item a restore brings back whose object is gone.
    Respawn,
}
