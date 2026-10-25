//! Test-only actor-free bindings and moves on the actual loadout table.
//!
//! These identities retain no resource and have no live provider or admission route.

use std::{fmt, num::NonZeroU64};

use pretty_assertions::assert_eq;

use crate::controllers::{
    frame::Hand,
    loadout::{
        BodyLayout, Socket,
        binding::Binding,
        item::{ItemId, ItemKind},
        table::{Departure, Docked, Item, ItemView, Loadout, Place},
        tests::fixtures::{a_pistol, check_invariants, object, take, weapon},
    },
};

/// A nonzero model identity, distinct from an object address and not a retained resource.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelLeaseId(NonZeroU64);

impl ModelLeaseId {
    pub const fn new(identity: NonZeroU64) -> Self {
        Self(identity)
    }
}

impl fmt::Display for ModelLeaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "model lease #{}", self.0)
    }
}

/// An actor-free render identity and movement revision within one table lifetime.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SafeItemBinding {
    pub model: ModelLeaseId,
    revision: u64,
}

impl SafeItemBinding {
    pub const fn revision(self) -> u64 {
        self.revision
    }
}

/// A copied validation view, without a borrow or resource-lifetime guarantee.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SafeItemView {
    pub item: ItemId,
    pub binding: SafeItemBinding,
    pub place: Place,
}

/// Refusals leave the actor-free item's identity, location, and model binding unchanged.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SafeItemError {
    UnknownItem(ItemId),
    StaleItem(ItemId),
    NotHeld(ItemId),
    ModelAlreadyBound { model: ModelLeaseId, item: ItemId },
    Occupied { place: Place, item: ItemId },
    UnsupportedHolster(Socket),
    LooseRepresentationUnavailable,
    RevisionExhausted,
}

impl fmt::Display for SafeItemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "loadout safe item: {self:?}")
    }
}

impl std::error::Error for SafeItemError {}

impl Loadout {
    /// Resolve an actor-free item from the real table, including its current hand or holster.
    pub fn safe_item(&self, id: ItemId) -> Option<SafeItemView> {
        self.items()
            .find(|item| item.id == id)
            .and_then(SafeItemView::from_item)
    }

    /// Resolve a model identity through its current table owner.
    pub fn safe_item_for_model(&self, model: ModelLeaseId) -> Option<SafeItemView> {
        self.items()
            .filter_map(SafeItemView::from_item)
            .find(|item| item.binding.model == model)
    }

    /// Admit a controlled actor-free item without a model allocation or native actor.
    pub fn admit_safe_item(
        &mut self,
        model: ModelLeaseId,
        hand: Hand,
    ) -> Result<SafeItemView, SafeItemError> {
        if let Some(owner) = self.safe_item_for_model(model) {
            return Err(SafeItemError::ModelAlreadyBound {
                model,
                item: owner.item,
            });
        }
        self.require_safe_destination(Place::Hand(hand))?;
        let id = self.allocate();
        let binding = SafeItemBinding { model, revision: 0 };
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

    /// Draw, transfer, or stow the same item after checking its complete current view.
    pub fn move_safe_item(
        &mut self,
        expected: SafeItemView,
        destination: Place,
    ) -> Result<SafeItemView, SafeItemError> {
        let current = self
            .safe_item(expected.item)
            .ok_or(SafeItemError::UnknownItem(expected.item))?;
        if current != expected {
            return Err(SafeItemError::StaleItem(expected.item));
        }
        if matches!(destination, Place::World(_)) {
            return Err(SafeItemError::LooseRepresentationUnavailable);
        }
        if destination == current.place {
            return Ok(current);
        }
        if matches!(
            (current.place, destination),
            (Place::Socket(_), Place::Socket(_))
        ) {
            return Err(SafeItemError::NotHeld(expected.item));
        }
        self.require_safe_destination(destination)?;
        let revision = current
            .binding
            .revision
            .checked_add(1)
            .ok_or(SafeItemError::RevisionExhausted)?;
        let mut item = match current.place {
            Place::Hand(hand) => self.hands[hand.index()].take(),
            Place::Socket(socket) => self.sockets[socket.index()]
                .take()
                .map(|docked| docked.item),
            Place::World(_) => None,
        }
        .ok_or(SafeItemError::StaleItem(expected.item))?;
        let binding = SafeItemBinding {
            revision,
            ..current.binding
        };
        item.binding = Binding::SafeItem(binding);
        match destination {
            Place::Hand(hand) => self.hands[hand.index()] = Some(item),
            Place::Socket(socket) => self.sockets[socket.index()] = Some(Docked { item }),
            Place::World(_) => unreachable!("world destinations are refused before removal"),
        }
        Ok(SafeItemView {
            item: current.item,
            binding,
            place: destination,
        })
    }

    /// Remove a logical item and return its model identity without claiming resource cleanup.
    pub fn retire_safe_item(
        &mut self,
        expected: SafeItemView,
    ) -> Result<ModelLeaseId, SafeItemError> {
        let current = self
            .safe_item(expected.item)
            .ok_or(SafeItemError::UnknownItem(expected.item))?;
        if current != expected {
            return Err(SafeItemError::StaleItem(expected.item));
        }
        match current.place {
            Place::Hand(hand) => self.hands[hand.index()] = None,
            Place::Socket(socket) => self.sockets[socket.index()] = None,
            Place::World(_) => return Err(SafeItemError::LooseRepresentationUnavailable),
        }
        Ok(current.binding.model)
    }

    fn require_safe_destination(&self, destination: Place) -> Result<(), SafeItemError> {
        match destination {
            Place::Socket(socket)
                if !self.body.has(socket) || !matches!(socket, Socket::Hip(_) | Socket::Chest) =>
            {
                return Err(SafeItemError::UnsupportedHolster(socket));
            }
            Place::World(_) => return Err(SafeItemError::LooseRepresentationUnavailable),
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
        if item.kind != ItemKind::Grenade || matches!(item.place, Place::World(_)) {
            return None;
        }
        Some(Self {
            item: item.id,
            binding: item.binding.safe_item()?,
            place: item.place,
        })
    }
}

#[test]
fn independent_safe_items_resolve_through_draw_transfer_and_stow() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let second = table.admit_safe_item(model(2), Hand::Right).unwrap();
    assert_ne!(first.item, second.item);
    assert_eq!(table.primary(), None);
    assert_eq!(table.snapshot().items.len(), 2);
    check_invariants(&table);
    check_safe_items(&table);

    let stowed = table
        .move_safe_item(first, Place::Socket(Socket::Hip(Hand::Left)))
        .unwrap();
    assert_eq!(table.safe_item(first.item), Some(stowed));
    assert_eq!(table.safe_item_for_model(model(2)), Some(second));
    check_safe_items(&table);
    let drawn = table
        .move_safe_item(stowed, Place::Hand(Hand::Left))
        .unwrap();
    let second = table
        .move_safe_item(second, Place::Socket(Socket::Chest))
        .unwrap();
    let transferred = table
        .move_safe_item(drawn, Place::Hand(Hand::Right))
        .unwrap();
    assert_eq!(transferred.item, first.item);
    assert_eq!(transferred.binding.model, first.binding.model);
    assert_eq!(transferred.binding.revision(), 3);
    assert_eq!(table.safe_item_for_model(model(1)), Some(transferred));
    assert_eq!(table.safe_item(second.item), Some(second));
    assert_eq!(table.held(Hand::Left), None);
    check_safe_items(&table);
}

#[test]
fn duplicate_lease_occupied_hand_and_invalid_holster_leave_the_table_unchanged() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let second = table.admit_safe_item(model(2), Hand::Right).unwrap();
    let before = table.snapshot();
    let next_id = table.next_id;
    assert_eq!(
        table.admit_safe_item(model(1), Hand::Right),
        Err(SafeItemError::ModelAlreadyBound {
            model: model(1),
            item: first.item
        })
    );
    assert_eq!(
        table.admit_safe_item(model(3), Hand::Right),
        Err(SafeItemError::Occupied {
            place: second.place,
            item: second.item
        })
    );
    assert_eq!(
        table.move_safe_item(first, second.place),
        Err(SafeItemError::Occupied {
            place: second.place,
            item: second.item
        })
    );
    assert_eq!(
        table.move_safe_item(first, Place::Socket(Socket::Shoulder(Hand::Left))),
        Err(SafeItemError::UnsupportedHolster(Socket::Shoulder(
            Hand::Left
        )))
    );
    assert_eq!(table.snapshot(), before);
    assert_eq!(table.next_id, next_id);
    check_safe_items(&table);
}

#[test]
fn safe_items_share_native_occupancy_without_rebinding_or_replacing_a_weapon() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let native = take(&mut table, Hand::Right, 1, weapon(a_pistol()));
    assert_eq!(table.safe_item(native), None);
    let before = table.snapshot();
    assert_eq!(
        table.admit_safe_item(model(1), Hand::Right),
        Err(SafeItemError::Occupied {
            place: Place::Hand(Hand::Right),
            item: native
        })
    );
    assert_eq!(table.snapshot(), before);
    let safe = table.admit_safe_item(model(1), Hand::Left).unwrap();
    assert_eq!(table.primary(), Some(Hand::Right));
    check_invariants(&table);
    table.dock(Hand::Right, Socket::Chest).unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.move_safe_item(safe, Place::Socket(Socket::Chest)),
        Err(SafeItemError::Occupied {
            place: Place::Socket(Socket::Chest),
            item: native
        })
    );
    assert_eq!(table.snapshot(), before);
    assert_eq!(
        table
            .items()
            .find(|item| item.id == native)
            .unwrap()
            .binding,
        Binding::Live(object(1))
    );
    assert_eq!(table.safe_item_for_model(model(1)), Some(safe));
    check_invariants(&table);
    let stowed = table
        .move_safe_item(safe, Place::Socket(Socket::Hip(Hand::Left)))
        .unwrap();
    table.draw(Socket::Chest, Hand::Right).unwrap();
    assert_eq!(
        table.held(Hand::Right).unwrap().binding,
        Binding::Live(object(1))
    );
    assert_eq!(table.safe_item(safe.item), Some(stowed));
    check_safe_items(&table);
}

#[test]
fn stale_views_cannot_move_or_retire_an_item_after_it_returns_to_the_same_hand() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let original = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let moved = table
        .move_safe_item(original, Place::Hand(Hand::Right))
        .unwrap();
    let current = table.move_safe_item(moved, original.place).unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.move_safe_item(original, Place::Socket(Socket::Chest)),
        Err(SafeItemError::StaleItem(original.item))
    );
    assert_eq!(
        table.retire_safe_item(original),
        Err(SafeItemError::StaleItem(original.item))
    );
    assert_eq!(table.snapshot(), before);
    assert_eq!(table.safe_item(original.item), Some(current));
    check_safe_items(&table);
}

#[test]
fn release_requires_a_loose_representation_and_retirement_invalidates_only_that_identity() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let second = table.admit_safe_item(model(2), Hand::Right).unwrap();
    let before = table.snapshot();
    for departure in [Departure::Dropped, Departure::AtSpeed, Departure::Returned] {
        assert_eq!(
            table.move_safe_item(first, Place::World(departure)),
            Err(SafeItemError::LooseRepresentationUnavailable)
        );
        assert_eq!(table.snapshot(), before);
    }
    assert_eq!(table.retire_safe_item(first), Ok(model(1)));
    assert_eq!(table.safe_item(first.item), None);
    assert_eq!(table.safe_item_for_model(model(1)), None);
    assert_eq!(table.safe_item(second.item), Some(second));
    assert_eq!(
        table.retire_safe_item(first),
        Err(SafeItemError::UnknownItem(first.item))
    );
    assert_eq!(
        table.move_safe_item(first, Place::Hand(Hand::Left)),
        Err(SafeItemError::UnknownItem(first.item))
    );
    let replacement = table.admit_safe_item(model(3), Hand::Left).unwrap();
    assert_ne!(replacement.item, first.item);
    let second = table
        .move_safe_item(second, Place::Socket(Socket::Chest))
        .unwrap();
    assert_eq!(table.retire_safe_item(second), Ok(model(2)));
    assert_eq!(table.safe_item_for_model(model(2)), None);
    assert_eq!(table.safe_item(replacement.item), Some(replacement));
    check_safe_items(&table);
}

#[test]
fn missing_holster_and_exhausted_revision_do_not_move_the_item() {
    let mut table = Loadout::new(BodyLayout::new(Hand::Right, 0, 0, false));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.move_safe_item(first, Place::Socket(Socket::Chest)),
        Err(SafeItemError::UnsupportedHolster(Socket::Chest))
    );
    assert_eq!(table.snapshot(), before);
    let Some(item) = table.hands[Hand::Left.index()].as_mut() else {
        panic!("held item")
    };
    let Binding::SafeItem(binding) = &mut item.binding else {
        panic!("safe binding")
    };
    binding.revision = u64::MAX;
    let current = table.safe_item(first.item).unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.move_safe_item(current, Place::Hand(Hand::Right)),
        Err(SafeItemError::RevisionExhausted)
    );
    assert_eq!(table.snapshot(), before);
    assert_eq!(table.move_safe_item(current, current.place), Ok(current));
    check_safe_items(&table);
}

#[test]
fn a_holstered_item_must_be_drawn_before_restowing_and_can_leave_a_removed_holster() {
    let mut table = Loadout::new(BodyLayout::full(Hand::Right));
    let first = table.admit_safe_item(model(1), Hand::Left).unwrap();
    let stowed = table
        .move_safe_item(first, Place::Socket(Socket::Hip(Hand::Left)))
        .unwrap();
    let before = table.snapshot();
    assert_eq!(
        table.move_safe_item(stowed, Place::Socket(Socket::Chest)),
        Err(SafeItemError::NotHeld(first.item))
    );
    assert_eq!(table.snapshot(), before);
    table.set_body(BodyLayout::new(Hand::Right, 0, 0, false));
    let drawn = table
        .move_safe_item(stowed, Place::Hand(Hand::Right))
        .unwrap();
    assert_eq!(drawn.item, first.item);
    assert_eq!(drawn.binding.model, first.binding.model);
    check_invariants(&table);
    check_safe_items(&table);
}

fn model(n: u64) -> ModelLeaseId {
    ModelLeaseId::new(NonZeroU64::new(n).unwrap())
}

fn check_safe_items(table: &Loadout) {
    let items: Vec<_> = table.items().collect();
    for (index, item) in items.iter().enumerate() {
        assert!(items[..index].iter().all(|other| other.id != item.id));
        if !matches!(item.place, Place::World(_)) {
            assert!(items[..index].iter().all(|other| other.place != item.place));
        }
        let Some(safe) = table.safe_item(item.id) else {
            continue;
        };
        assert_eq!(item.kind, ItemKind::Grenade);
        assert_eq!(item.binding.address(), None);
        assert_eq!(item.binding.safe_item(), Some(safe.binding));
        assert_eq!(safe.place, item.place);
        assert_eq!(table.safe_item_for_model(safe.binding.model), Some(safe));
        assert!(items[..index].iter().all(|other| {
            other
                .binding
                .safe_item()
                .is_none_or(|binding| binding.model != safe.binding.model)
        }));
        assert_eq!(table.snapshot().projection.slot_of(item.id), None);
        match safe.place {
            Place::Hand(_) | Place::Socket(Socket::Hip(_) | Socket::Chest) => {}
            _ => panic!("safe item outside a hand or a holster"),
        }
    }
}
