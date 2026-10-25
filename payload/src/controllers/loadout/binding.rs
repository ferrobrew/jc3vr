//! The live-object binding beside the table's item identity.
//!
//! A native binding can await a replacement object across a spawn reset. Bindings establish
//! neither engine liveness nor resource ownership.

use std::{fmt, num::NonZeroUsize};

use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::loadout::item::slot_label;
#[cfg(test)]
use crate::controllers::loadout::tests::controlled_safe_binding::SafeItemBinding;

/// A live game object's address: a weapon, a prop's rigid object, or a held grenade or charge.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectAddress(NonZeroUsize);

impl ObjectAddress {
    pub const fn new(address: NonZeroUsize) -> ObjectAddress {
        ObjectAddress(address)
    }

    /// The object at `address`, or `None` for a null one.
    pub const fn from_address(address: usize) -> Option<ObjectAddress> {
        match NonZeroUsize::new(address) {
            Some(address) => Some(ObjectAddress(address)),
            None => None,
        }
    }

    pub const fn get(self) -> usize {
        self.0.get()
    }
}

impl fmt::Display for ObjectAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

/// The live object an item is bound to, or what it waits for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Binding {
    /// A live object.
    Live(ObjectAddress),
    /// None yet: the item waits for the game's object of its type.
    Awaiting(Awaiting),
    /// A controlled actor-free identity used by the real-table tests.
    #[cfg(test)]
    SafeItem(SafeItemBinding),
}

impl Binding {
    /// The live object's address, while bound to one.
    pub const fn address(self) -> Option<ObjectAddress> {
        match self {
            Binding::Live(address) => Some(address),
            Binding::Awaiting(_) => None,
            #[cfg(test)]
            Binding::SafeItem(_) => None,
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
            #[cfg(test)]
            Binding::SafeItem(binding) => write!(f, "safe item, {}", binding.model),
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
