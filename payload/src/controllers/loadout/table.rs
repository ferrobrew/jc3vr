//! The loadout table: every item the local player has, each in exactly one place. Pure.
//!
//! The invariants are carried by the layout of the table rather than checked after the fact:
//!
//! - an [`Item`] is a value with no copy or clone, owned by exactly one container (a hand, a
//!   socket, or the world), so it is in exactly one place;
//! - each hand and each socket is an `Option`, so it holds at most one item;
//! - a socket's item is a [`Docked`], made only by [`Docked::new`], which refuses an item the
//!   socket does not fit, so every socket holds an item of its size class or smaller;
//! - the world holds only [`Lying`] items, made only from weapons, since a prop, a grenade, or a
//!   charge let go of leaves the table;
//! - identities come only from the table's counter, so no two items share one;
//! - the primary hand is never stored: [`primary`] derives it from the hands each time.
//!
//! What the types do not carry, every transition keeps: no two live items share an object
//! address, and a transition that fails leaves the table as it was.

use std::fmt;

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::Binding,
        item::{ItemId, ItemKind},
        projection::{Projection, project},
        reset::Caches,
        socket::{BodyLayout, Socket},
    },
};

/// The table.
#[derive(Debug)]
pub struct Loadout {
    pub(super) body: BodyLayout,
    pub(super) hands: [Option<Item>; 2],
    pub(super) sockets: [Option<Docked>; Socket::COUNT],
    pub(super) world: Vec<Lying>,
    pub(super) next_id: ItemId,
    pub(super) caches: Caches,
}

impl Loadout {
    /// An empty table for a body of `body`'s sockets.
    pub const fn new(body: BodyLayout) -> Loadout {
        Loadout {
            body,
            hands: [None, None],
            sockets: [const { None }; Socket::COUNT],
            world: Vec::new(),
            next_id: ItemId::FIRST,
            caches: Caches::new(),
        }
    }

    pub const fn body(&self) -> BodyLayout {
        self.body
    }

    /// Change the body's sockets or handedness. An occupied socket the new layout lacks keeps its
    /// item until the player draws it, since nothing moves an item but the player; only where new
    /// items may go changes.
    pub fn set_body(&mut self, body: BodyLayout) {
        self.body = body;
    }

    /// The hand that the game's wielded-weapon consumers follow: see [`primary`].
    pub fn primary(&self) -> Option<Hand> {
        primary(
            self.hands.each_ref().map(|h| h.as_ref().map(|i| i.kind)),
            self.body.dominant(),
        )
    }

    /// Every item and where it is: the hands, then the sockets, then the world.
    pub fn items(&self) -> impl Iterator<Item = ItemView> + '_ {
        let hands = Hand::BOTH.into_iter().filter_map(|hand| {
            self.hands[hand.index()]
                .as_ref()
                .map(|item| item.view(Place::Hand(hand)))
        });
        let sockets = Socket::ALL.into_iter().filter_map(|socket| {
            self.sockets[socket.index()]
                .as_ref()
                .map(|docked| docked.item.view(Place::Socket(socket)))
        });
        let world = self
            .world
            .iter()
            .map(|lying| lying.item.view(Place::World(lying.departure)));
        hands.chain(sockets).chain(world)
    }

    /// The item in `hand`.
    pub fn held(&self, hand: Hand) -> Option<ItemView> {
        self.hands[hand.index()]
            .as_ref()
            .map(|item| item.view(Place::Hand(hand)))
    }

    /// What the rest of the mod reads: the items, the primary hand, and the slot projection, as of
    /// now.
    pub fn snapshot(&self) -> LoadoutSnapshot {
        LoadoutSnapshot {
            body: self.body,
            items: self.items().collect(),
            primary: self.primary(),
            projection: project(self),
        }
    }

    /// Hand out the next identity.
    pub(super) fn allocate(&mut self) -> ItemId {
        let id = self.next_id;
        self.next_id = id.next();
        id
    }
}

/// The primary hand, which the game's wielded-weapon consumers follow: the dominant hand when it
/// holds a weapon, otherwise the other hand when it does, otherwise none. `hands` is what each
/// hand holds, by [`Hand::index`].
pub fn primary(hands: [Option<ItemKind>; 2], dominant: Hand) -> Option<Hand> {
    let holds_weapon = |hand: Hand| matches!(hands[hand.index()], Some(ItemKind::Weapon(_)));
    [dominant, dominant.other()]
        .into_iter()
        .find(|&hand| holds_weapon(hand))
}

/// Where an item is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Place {
    Hand(Hand),
    Socket(Socket),
    /// Lying in the world, kept near the player, and how it came to be there.
    World(Departure),
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Place::Hand(hand) => write!(f, "the {} hand", hand.label()),
            Place::Socket(socket) => socket.fmt(f),
            Place::World(departure) => write!(f, "the world ({departure})"),
        }
    }
}

/// How a weapon in the world came to be there.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Departure {
    /// A hand let go of it while the player moved at an ordinary pace: the player chose to leave
    /// it, so it never comes back by itself.
    Dropped,
    /// A hand let go of it while the player travelled fast (the wingsuit, a vehicle, the
    /// parachute, or a fall): the player never chose to leave it where it lands, so it comes back
    /// once it is left behind.
    AtSpeed,
    /// It was lost and no socket that fits it was free, so it was brought to the player's feet.
    Returned,
}

impl fmt::Display for Departure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Departure::Dropped => "dropped",
            Departure::AtSpeed => "let go of at speed",
            Departure::Returned => "brought back to the player",
        })
    }
}

/// One item, as the rest of the mod reads it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ItemView {
    pub id: ItemId,
    pub kind: ItemKind,
    pub binding: Binding,
    pub place: Place,
}

/// The table as the rest of the mod reads it, published each tick.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadoutSnapshot {
    pub body: BodyLayout,
    pub items: Vec<ItemView>,
    pub primary: Option<Hand>,
    /// What the game's slots should hold now.
    pub projection: Projection,
}

impl LoadoutSnapshot {
    /// The item in `hand`.
    pub fn held(&self, hand: Hand) -> Option<&ItemView> {
        self.items
            .iter()
            .find(|item| item.place == Place::Hand(hand))
    }

    /// The item on `socket`.
    pub fn docked(&self, socket: Socket) -> Option<&ItemView> {
        self.items
            .iter()
            .find(|item| item.place == Place::Socket(socket))
    }
}

/// One item in the table. Deliberately neither `Copy` nor `Clone`: it is moved between the
/// containers, never duplicated.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Item {
    pub(super) id: ItemId,
    pub(super) kind: ItemKind,
    pub(super) binding: Binding,
}

impl Item {
    pub(super) fn view(&self, place: Place) -> ItemView {
        ItemView {
            id: self.id,
            kind: self.kind,
            binding: self.binding,
            place,
        }
    }
}

/// An item on a socket that fits it.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Docked {
    pub(super) item: Item,
}

impl Docked {
    /// Dock `item` on `socket`, or hand it back when the socket does not take it: only a weapon
    /// of the socket's size class or smaller docks.
    pub(super) fn new(item: Item, socket: Socket) -> Result<Docked, Item> {
        match item.kind {
            ItemKind::Weapon(weapon) if socket.fits(weapon.size()) => Ok(Docked { item }),
            _ => Err(item),
        }
    }
}

/// A weapon lying in the world.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Lying {
    pub(super) item: Item,
    pub(super) departure: Departure,
}

impl Lying {
    /// Lay `item` in the world, or hand it back when it is not a weapon.
    pub(super) fn new(item: Item, departure: Departure) -> Result<Lying, Item> {
        match item.kind {
            ItemKind::Weapon(_) => Ok(Lying { item, departure }),
            _ => Err(item),
        }
    }
}
