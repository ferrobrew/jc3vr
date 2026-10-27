//! Grenade item bindings and checked moves on the runtime loadout table.
//!
//! The grenade controller retains the item's native resource. These identifiers never stand in for
//! a native address, and moving an item never creates or activates an engine object.

use std::{fmt, num::NonZeroU64};

use crate::controllers::{
    frame::Hand,
    loadout::{
        Socket,
        binding::Binding,
        item::{ItemId, ItemKind},
        table::{Departure, Docked, Item, ItemView, Loadout, Place},
    },
};

/// An opaque item lease identity, distinct from an object address.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemLeaseId(NonZeroU64);

impl ItemLeaseId {
    pub const fn new(identity: NonZeroU64) -> Self {
        Self(identity)
    }
}

impl fmt::Display for ItemLeaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "item lease #{}", self.0)
    }
}

/// An item's runtime identity and movement revision within one table lifetime.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SafeItemBinding {
    pub lease: ItemLeaseId,
    revision: u64,
}

impl SafeItemBinding {
    #[cfg(test)]
    pub const fn revision(self) -> u64 {
        self.revision
    }

    #[cfg(test)]
    pub(crate) fn exhaust_revision(&mut self) {
        self.revision = u64::MAX;
    }
}

/// A copied validation view, without a resource-lifetime guarantee.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SafeItemView {
    pub item: ItemId,
    pub binding: SafeItemBinding,
    pub place: Place,
}

/// Refusals leave the item's identity, place, and model binding unchanged.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SafeItemError {
    NotRunning,
    Resetting,
    UnknownItem(ItemId),
    StaleItem(ItemId),
    NotHeld(ItemId),
    LeaseAlreadyBound { model: ItemLeaseId, item: ItemId },
    Occupied { place: Place, item: ItemId },
    UnsupportedHolster(Socket),
    UnsupportedDeparture(Departure),
    RevisionExhausted,
}

impl fmt::Display for SafeItemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "loadout safe item: {self:?}")
    }
}

impl std::error::Error for SafeItemError {}

impl Loadout {
    /// Resolve an item in a hand, a holster, or the inert world models.
    pub fn safe_item(&self, id: ItemId) -> Option<SafeItemView> {
        self.safe_items().find(|item| item.item == id)
    }

    pub fn safe_items(&self) -> impl Iterator<Item = SafeItemView> + '_ {
        self.items().filter_map(SafeItemView::from_item)
    }

    /// Resolve a model identity through its current table owner.
    pub fn safe_item_for_lease(&self, model: ItemLeaseId) -> Option<SafeItemView> {
        self.safe_items().find(|item| item.binding.lease == model)
    }

    /// Admit an item without allocating its native actor.
    pub fn admit_safe_item(
        &mut self,
        model: ItemLeaseId,
        hand: Hand,
    ) -> Result<SafeItemView, SafeItemError> {
        if let Some(owner) = self.safe_item_for_lease(model) {
            return Err(SafeItemError::LeaseAlreadyBound {
                model,
                item: owner.item,
            });
        }
        self.require_safe_destination(Place::Hand(hand))?;
        let id = self.allocate();
        let binding = SafeItemBinding {
            lease: model,
            revision: 0,
        };
        self.hands[hand.index()] = Some(Item {
            id,
            kind: ItemKind::Grenade,
            binding: Binding::SafeItem(binding),
        });
        Ok(SafeItemView {
            item: id,
            binding,
            place: Place::Hand(hand),
        })
    }

    /// Draw, transfer, stow, or drop the same item after checking its complete current view.
    pub fn move_safe_item(
        &mut self,
        expected: SafeItemView,
        destination: Place,
    ) -> Result<SafeItemView, SafeItemError> {
        let current = self.require_safe_view(expected)?;
        if destination == current.place {
            return Ok(current);
        }
        if !matches!(current.place, Place::Hand(_)) && !matches!(destination, Place::Hand(_)) {
            return Err(SafeItemError::NotHeld(expected.item));
        }
        self.require_safe_destination(destination)?;
        let revision = current
            .binding
            .revision
            .checked_add(1)
            .ok_or(SafeItemError::RevisionExhausted)?;
        let mut item = self
            .remove_safe(current)
            .ok_or(SafeItemError::StaleItem(expected.item))?;
        let binding = SafeItemBinding {
            revision,
            ..current.binding
        };
        item.binding = Binding::SafeItem(binding);
        match destination {
            Place::Hand(hand) => self.hands[hand.index()] = Some(item),
            Place::Socket(socket) => self.sockets[socket.index()] = Some(Docked { item }),
            Place::World(_) => self.safe_world.push(SafeWorld { item }),
        }
        Ok(SafeItemView {
            item: current.item,
            binding,
            place: destination,
        })
    }

    /// Return the model identity to its owner for resource cleanup or native launch completion.
    pub fn retire_safe_item(
        &mut self,
        expected: SafeItemView,
    ) -> Result<ItemLeaseId, SafeItemError> {
        let current = self.require_safe_view(expected)?;
        self.remove_safe(current)
            .ok_or(SafeItemError::StaleItem(expected.item))?;
        Ok(current.binding.lease)
    }

    fn require_safe_view(&self, expected: SafeItemView) -> Result<SafeItemView, SafeItemError> {
        let current = self
            .safe_item(expected.item)
            .ok_or(SafeItemError::UnknownItem(expected.item))?;
        if current != expected {
            return Err(SafeItemError::StaleItem(expected.item));
        }
        Ok(current)
    }

    fn remove_safe(&mut self, current: SafeItemView) -> Option<Item> {
        match current.place {
            Place::Hand(hand) => self.hands[hand.index()].take(),
            Place::Socket(socket) => self.sockets[socket.index()]
                .take()
                .map(|docked| docked.item),
            Place::World(_) => {
                let index = self
                    .safe_world
                    .iter()
                    .position(|lying| lying.item.id == current.item)?;
                Some(self.safe_world.remove(index).item)
            }
        }
    }

    fn require_safe_destination(&self, destination: Place) -> Result<(), SafeItemError> {
        match destination {
            Place::Socket(socket)
                if !self.body.has(socket) || !matches!(socket, Socket::Hip(_) | Socket::Chest) =>
            {
                return Err(SafeItemError::UnsupportedHolster(socket));
            }
            Place::World(Departure::Dropped) => return Ok(()),
            Place::World(departure) => return Err(SafeItemError::UnsupportedDeparture(departure)),
            Place::Hand(_) | Place::Socket(_) => {}
        }
        if let Some(item) = self.items().find(|item| item.place == destination) {
            return Err(SafeItemError::Occupied {
                place: destination,
                item: item.id,
            });
        }
        Ok(())
    }
}

impl Binding {
    pub const fn safe_item(self) -> Option<SafeItemBinding> {
        match self {
            Binding::SafeItem(binding) => Some(binding),
            Binding::Live(_) | Binding::Awaiting(_) => None,
        }
    }
}

impl SafeItemView {
    fn from_item(item: ItemView) -> Option<Self> {
        if item.kind != ItemKind::Grenade {
            return None;
        }
        Some(Self {
            item: item.id,
            binding: item.binding.safe_item()?,
            place: item.place,
        })
    }
}

/// An item lying in the world, separate from native physical weapons.
#[derive(Debug)]
pub(super) struct SafeWorld {
    pub(super) item: Item,
}
