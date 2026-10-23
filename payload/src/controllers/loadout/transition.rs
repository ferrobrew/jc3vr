//! The player's and the game's moves of items: what each takes, what it does to the table, and
//! why it can be refused. Pure.
//!
//! Every transition either applies whole and returns what changed, each change with its cause for
//! the log, or is refused with a [`TransitionError`] and leaves the table as it was. None moves an
//! item the player did not move, except where the game's word on ownership decides (a give, a
//! removal) or a weapon was lost rather than dropped ([`Loadout::lose`]).

use std::fmt;

use crate::controllers::{
    frame::Hand,
    loadout::{
        binding::{Awaiting, Binding, ObjectAddress},
        item::{ItemId, ItemKind, WeaponType},
        reset::{CacheKind, ResetKind},
        socket::Socket,
        table::{Departure, Docked, Item, Loadout, Lying, Place},
    },
};

/// A take into a hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Take {
    pub hand: Hand,
    pub object: ObjectAddress,
    pub kind: ItemKind,
    pub source: TakeSource,
}

/// Where a take comes from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TakeSource {
    /// Off the ground: a weapon, or a prop.
    World,
    /// Off a rack that hands its own weapon over.
    Rack,
    /// A rack's copy, spawned for a rack with unlimited stock.
    RackCopy,
    /// Out of a locker.
    Locker,
    /// Out of a living character's hands.
    Character,
    /// A mission's give, taken by the hand it was offered to.
    Give,
    /// The grapple snatch.
    Snatch,
    /// From a pouch: a grenade or a C4 charge.
    Pouch,
    /// A live grenade caught out of the air.
    Catch,
}

impl TakeSource {
    pub const ALL: [TakeSource; 9] = [
        TakeSource::World,
        TakeSource::Rack,
        TakeSource::RackCopy,
        TakeSource::Locker,
        TakeSource::Character,
        TakeSource::Give,
        TakeSource::Snatch,
        TakeSource::Pouch,
        TakeSource::Catch,
    ];

    /// Whether a take from here can bring an item of `kind`.
    pub const fn admits(self, kind: ItemKind) -> bool {
        match self {
            TakeSource::World => matches!(kind, ItemKind::Weapon(_) | ItemKind::Prop),
            TakeSource::Rack
            | TakeSource::RackCopy
            | TakeSource::Locker
            | TakeSource::Character
            | TakeSource::Give
            | TakeSource::Snatch => matches!(kind, ItemKind::Weapon(_)),
            TakeSource::Pouch => matches!(kind, ItemKind::Grenade | ItemKind::Charge),
            TakeSource::Catch => matches!(kind, ItemKind::Grenade),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            TakeSource::World => "taken off the ground",
            TakeSource::Rack => "taken off a rack",
            TakeSource::RackCopy => "a rack's copy taken",
            TakeSource::Locker => "taken out of a locker",
            TakeSource::Character => "taken from a character's hands",
            TakeSource::Give => "a mission's give taken",
            TakeSource::Snatch => "snatched with the grapple",
            TakeSource::Pouch => "taken from a pouch",
            TakeSource::Catch => "caught",
        }
    }
}

/// How a hand let go.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Release {
    /// At an ordinary pace.
    Dropped,
    /// While the player travelled fast.
    AtSpeed,
}

impl Release {
    pub const ALL: [Release; 2] = [Release::Dropped, Release::AtSpeed];

    const fn departure(self) -> Departure {
        match self {
            Release::Dropped => Departure::Dropped,
            Release::AtSpeed => Departure::AtSpeed,
        }
    }
}

/// Why a weapon in the world counts as lost.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LostWhy {
    /// It fell through the terrain or below the world's floor, or sank out of reach in water.
    OutOfWorld,
    /// It was let go of at speed and the player has left it behind.
    LeftBehind,
}

impl LostWhy {
    pub const ALL: [LostWhy; 2] = [LostWhy::OutOfWorld, LostWhy::LeftBehind];

    pub const fn label(self) -> &'static str {
        match self {
            LostWhy::OutOfWorld => "lost out of the world",
            LostWhy::LeftBehind => "left behind at speed",
        }
    }
}

/// A weapon the game gives the player.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Give {
    pub object: ObjectAddress,
    pub weapon: WeaponType,
    pub source: GiveSource,
}

/// What gives a weapon.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GiveSource {
    /// A mission's give or give-back.
    Mission,
    /// A challenge's loadout.
    Challenge,
    /// A supply drop.
    SupplyDrop,
    /// A new game's starting loadout.
    NewGame,
    /// A give the game's respawn table delivers whose caller is not told apart: a mission's, a
    /// challenge's, the graph script's, an event's, or a reward.
    Game,
    /// The copy a rack with unlimited stock gives the game's own pickup.
    RackCopy,
}

impl GiveSource {
    pub const ALL: [GiveSource; 6] = [
        GiveSource::Mission,
        GiveSource::Challenge,
        GiveSource::SupplyDrop,
        GiveSource::NewGame,
        GiveSource::Game,
        GiveSource::RackCopy,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            GiveSource::Mission => "a mission's give",
            GiveSource::Challenge => "a challenge's loadout",
            GiveSource::SupplyDrop => "a supply drop",
            GiveSource::NewGame => "a new game's loadout",
            GiveSource::Game => "the game's give",
            GiveSource::RackCopy => "a rack's copy",
        }
    }
}

/// A weapon the mod spawned to stand for one the player has: one of the sidecar's weapons after a
/// load, or the second pistol of a pair split in two. It comes onto the body where it was
/// recorded, since the hands come back empty after a load.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Restored {
    pub object: ObjectAddress,
    pub weapon: WeaponType,
    /// Where the weapon was, or goes.
    pub socket: Socket,
}

/// What a game-side removal takes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Removal {
    /// The item bound to this object.
    Object(ObjectAddress),
    /// This item, bound or not.
    Item(ItemId),
    /// Every weapon on the body, the hands' included. The weapons lying in the world stay: the game
    /// takes what the player holds, not what lies near them.
    AllWeapons,
}

/// Why the game takes an item away.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RemovalWhy {
    /// A mission took the player's weapons.
    Mission,
    /// A challenge took the player's weapons.
    Challenge,
    /// The object is gone: despawned, or a grenade or charge spent.
    Gone,
    /// It lay unpinned and far from the player, so it was left to the game.
    Forgotten,
    /// Another character took it.
    TakenByOther,
    /// Its spawn never came.
    NeverSpawned,
}

impl RemovalWhy {
    pub const ALL: [RemovalWhy; 6] = [
        RemovalWhy::Mission,
        RemovalWhy::Challenge,
        RemovalWhy::Gone,
        RemovalWhy::Forgotten,
        RemovalWhy::TakenByOther,
        RemovalWhy::NeverSpawned,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            RemovalWhy::Mission => "a mission took it",
            RemovalWhy::Challenge => "a challenge took it",
            RemovalWhy::Gone => "the object is gone",
            RemovalWhy::Forgotten => "left to the game",
            RemovalWhy::TakenByOther => "another character took it",
            RemovalWhy::NeverSpawned => "its spawn never came",
        }
    }
}

/// Why an item moved, for the log.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Cause {
    Take(TakeSource),
    Dock,
    Draw,
    HandOver,
    LetGo(Release),
    Lost(LostWhy),
    Given(GiveSource),
    /// A restored item's object was spawned.
    Spawned,
    Removed(RemovalWhy),
    /// The game let go of it.
    Dropped,
    /// The mod spawned it to stand for one the player has.
    Restored,
    /// The spawn reset is about to replace the slots' weapons.
    ResetBegan,
    Reset(ResetKind),
    Restore(CacheKind),
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cause::Take(source) => f.write_str(source.label()),
            Cause::Dock => f.write_str("docked"),
            Cause::Draw => f.write_str("drawn"),
            Cause::HandOver => f.write_str("handed over"),
            Cause::LetGo(Release::Dropped) => f.write_str("let go of"),
            Cause::LetGo(Release::AtSpeed) => f.write_str("let go of at speed"),
            Cause::Lost(why) => f.write_str(why.label()),
            Cause::Given(source) => f.write_str(source.label()),
            Cause::Spawned => f.write_str("spawned again"),
            Cause::Removed(why) => f.write_str(why.label()),
            Cause::Dropped => f.write_str("the game dropped it"),
            Cause::Restored => f.write_str("restored onto the body"),
            Cause::ResetBegan => f.write_str("the spawn reset began"),
            Cause::Reset(kind) => write!(f, "the spawn reset ({})", kind.label()),
            Cause::Restore(kind) => write!(f, "the {} restored", kind.label()),
        }
    }
}

/// What a transition did to one item.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Change {
    Entered {
        to: Place,
    },
    Moved {
        from: Place,
        to: Place,
    },
    Left {
        from: Place,
    },
    /// The item is bound to a new object.
    Bound {
        object: ObjectAddress,
    },
    /// The item lost its object and waits for another.
    Unbound {
        awaiting: Awaiting,
    },
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Change::Entered { to } => write!(f, "is in {to}"),
            Change::Moved { from, to } => write!(f, "moved from {from} to {to}"),
            Change::Left { from } => write!(f, "left the table from {from}"),
            Change::Bound { object } => write!(f, "is bound to object {object}"),
            Change::Unbound { awaiting } => write!(f, "is {}", Binding::Awaiting(*awaiting)),
        }
    }
}

/// One change to the table, with its cause.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Transition {
    pub item: ItemId,
    pub kind: ItemKind,
    pub change: Change,
    pub cause: Cause,
}

impl fmt::Display for Transition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "loadout: {} ({}) {}: {}",
            self.item, self.kind, self.change, self.cause
        )
    }
}

/// Why a transition was refused. The table is as it was.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TransitionError {
    HandEmpty(Hand),
    HandOccupied {
        hand: Hand,
        item: ItemId,
    },
    SocketEmpty(Socket),
    SocketOccupied {
        socket: Socket,
        item: ItemId,
    },
    /// The body has no such socket in its layout.
    SocketAbsent(Socket),
    /// The socket does not take an item of this kind: too big, or no weapon at all.
    DoesNotFit {
        socket: Socket,
        kind: ItemKind,
    },
    /// A take from `source` cannot bring an item of `kind`.
    SourceMismatch {
        source: TakeSource,
        kind: ItemKind,
    },
    /// The object is already an item of the table's, here.
    AlreadyInTable {
        item: ItemId,
        place: Place,
    },
    /// The object taken is a known item of another kind.
    KindMismatch {
        item: ItemId,
        known: ItemKind,
        taken: ItemKind,
    },
    UnknownObject(ObjectAddress),
    UnknownItem(ItemId),
    /// A lost weapon must be lying in the world.
    NotInWorld {
        item: ItemId,
        place: Place,
    },
    /// The weapon was dropped, not lost: the player chose to leave it.
    NotLost {
        item: ItemId,
        departure: Departure,
    },
    /// The item waits for no spawn.
    NotAwaiting(ItemId),
    /// Two of the game's slots were reported for one slot or one object.
    DuplicateSlotObject(ObjectAddress),
    /// No snapshot of this kind was taken.
    NoCache(CacheKind),
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransitionError::HandEmpty(hand) => {
                write!(f, "loadout: the {} hand holds nothing", hand.label())
            }
            TransitionError::HandOccupied { hand, item } => {
                write!(f, "loadout: the {} hand already holds {item}", hand.label())
            }
            TransitionError::SocketEmpty(socket) => {
                write!(f, "loadout: nothing is docked on {socket}")
            }
            TransitionError::SocketOccupied { socket, item } => {
                write!(f, "loadout: {item} is already docked on {socket}")
            }
            TransitionError::SocketAbsent(socket) => {
                write!(f, "loadout: the body has no socket at {socket}")
            }
            TransitionError::DoesNotFit { socket, kind } => {
                write!(f, "loadout: {socket} does not take {kind}")
            }
            TransitionError::SourceMismatch { source, kind } => {
                write!(f, "loadout: {kind} cannot be {}", source.label())
            }
            TransitionError::AlreadyInTable { item, place } => {
                write!(f, "loadout: the object is already {item}, in {place}")
            }
            TransitionError::KindMismatch { item, known, taken } => {
                write!(f, "loadout: the object is {item}, {known}, not {taken}")
            }
            TransitionError::UnknownObject(object) => {
                write!(f, "loadout: no item is bound to object {object}")
            }
            TransitionError::UnknownItem(item) => write!(f, "loadout: no item {item}"),
            TransitionError::NotInWorld { item, place } => {
                write!(f, "loadout: {item} is in {place}, not lying in the world")
            }
            TransitionError::NotLost { item, departure } => write!(
                f,
                "loadout: {item} was {departure}, so it stays where the player left it"
            ),
            TransitionError::NotAwaiting(item) => {
                write!(f, "loadout: {item} waits for no spawn")
            }
            TransitionError::DuplicateSlotObject(object) => write!(
                f,
                "loadout: object {object}, or its slot, was reported twice among the game's slots"
            ),
            TransitionError::NoCache(kind) => {
                write!(f, "loadout: no {} was taken", kind.label())
            }
        }
    }
}

impl std::error::Error for TransitionError {}

impl Loadout {
    /// Take an object into `take.hand`: a new item, or a known weapon lying in the world picked up
    /// again. The other hand is never touched.
    pub fn take(&mut self, take: Take) -> Result<Vec<Transition>, TransitionError> {
        self.require_empty(take.hand)?;
        let cause = Cause::Take(take.source);
        let found = self.locate(take.object);
        if let Some(found) = found {
            if !matches!(found.spot, Spot::World(_)) {
                return Err(found.already());
            }
            if found.kind != take.kind {
                return Err(TransitionError::KindMismatch {
                    item: found.id,
                    known: found.kind,
                    taken: take.kind,
                });
            }
        }
        if !take.source.admits(take.kind) {
            return Err(TransitionError::SourceMismatch {
                source: take.source,
                kind: take.kind,
            });
        }
        let (item, from) = match found {
            Some(found) => {
                let (item, from) = self.remove_at(found)?;
                (item, Some(from))
            }
            None => (
                Item {
                    id: self.allocate(),
                    kind: take.kind,
                    binding: Binding::Live(take.object),
                },
                None,
            ),
        };
        Ok(vec![self.put_in_hand(item, take.hand, from, cause)])
    }

    /// Dock what `hand` holds on `socket`.
    pub fn dock(&mut self, hand: Hand, socket: Socket) -> Result<Vec<Transition>, TransitionError> {
        if self.hands[hand.index()].is_none() {
            return Err(TransitionError::HandEmpty(hand));
        }
        if !self.body.has(socket) {
            return Err(TransitionError::SocketAbsent(socket));
        }
        self.require_free(socket)?;
        let item = self.hands[hand.index()]
            .take()
            .ok_or(TransitionError::HandEmpty(hand))?;
        match Docked::new(item, socket) {
            Ok(docked) => {
                let transition = change(
                    &docked.item,
                    Some(Place::Hand(hand)),
                    Place::Socket(socket),
                    Cause::Dock,
                );
                self.sockets[socket.index()] = Some(docked);
                Ok(vec![transition])
            }
            Err(item) => {
                let kind = item.kind;
                self.hands[hand.index()] = Some(item);
                Err(TransitionError::DoesNotFit { socket, kind })
            }
        }
    }

    /// Draw what is docked on `socket` into `hand`.
    pub fn draw(&mut self, socket: Socket, hand: Hand) -> Result<Vec<Transition>, TransitionError> {
        self.require_empty(hand)?;
        let docked = self.sockets[socket.index()]
            .take()
            .ok_or(TransitionError::SocketEmpty(socket))?;
        Ok(vec![self.put_in_hand(
            docked.item,
            hand,
            Some(Place::Socket(socket)),
            Cause::Draw,
        )])
    }

    /// Hand what `from` holds to the other hand, which must be empty.
    pub fn hand_over(&mut self, from: Hand) -> Result<Vec<Transition>, TransitionError> {
        let to = from.other();
        if self.hands[from.index()].is_none() {
            return Err(TransitionError::HandEmpty(from));
        }
        self.require_empty(to)?;
        let item = self.hands[from.index()]
            .take()
            .ok_or(TransitionError::HandEmpty(from))?;
        Ok(vec![self.put_in_hand(
            item,
            to,
            Some(Place::Hand(from)),
            Cause::HandOver,
        )])
    }

    /// Let go of what `hand` holds: a weapon stays where it falls, kept near the player; anything
    /// else leaves the table.
    pub fn let_go(
        &mut self,
        hand: Hand,
        release: Release,
    ) -> Result<Vec<Transition>, TransitionError> {
        let item = self.hands[hand.index()]
            .take()
            .ok_or(TransitionError::HandEmpty(hand))?;
        let from = Place::Hand(hand);
        let cause = Cause::LetGo(release);
        Ok(vec![match Lying::new(item, release.departure()) {
            Ok(lying) => {
                let transition = change(
                    &lying.item,
                    Some(from),
                    Place::World(lying.departure),
                    cause,
                );
                self.world.push(lying);
                transition
            }
            Err(item) => left(&item, from, cause),
        }])
    }

    /// The weapon bound to `object`, lying in the world, is lost rather than dropped: it returns to
    /// the first free socket that fits it, or, with none free, to the player's feet. A weapon the
    /// player dropped at an ordinary pace is never left behind, only lost out of the world.
    pub fn lose(
        &mut self,
        object: ObjectAddress,
        why: LostWhy,
    ) -> Result<Vec<Transition>, TransitionError> {
        let found = self
            .locate(object)
            .ok_or(TransitionError::UnknownObject(object))?;
        let Place::World(departure) = found.place else {
            return Err(TransitionError::NotInWorld {
                item: found.id,
                place: found.place,
            });
        };
        if why == LostWhy::LeftBehind && departure != Departure::AtSpeed {
            return Err(TransitionError::NotLost {
                item: found.id,
                departure,
            });
        }
        let (item, from) = self.remove_at(found)?;
        Ok(self
            .onto_body(item, Some(from), Cause::Lost(why))
            .into_iter()
            .collect())
    }

    /// The game gives a weapon: it goes to an empty hand if one is free (the dominant first),
    /// otherwise to a free socket that fits it, otherwise to the player's feet.
    pub fn give(&mut self, give: Give) -> Result<Vec<Transition>, TransitionError> {
        if let Some(found) = self.locate(give.object) {
            return Err(found.already());
        }
        let item = Item {
            id: self.allocate(),
            kind: ItemKind::Weapon(give.weapon),
            binding: Binding::Live(give.object),
        };
        Ok(self
            .give_item(item, None, Cause::Given(give.source))
            .into_iter()
            .collect())
    }

    /// A restored item's object came: bind the item waiting for it.
    pub fn spawned(
        &mut self,
        item: ItemId,
        object: ObjectAddress,
    ) -> Result<Vec<Transition>, TransitionError> {
        if let Some(found) = self.locate(object) {
            return Err(found.already());
        }
        let entry = self
            .item_mut(item)
            .ok_or(TransitionError::UnknownItem(item))?;
        if !matches!(entry.binding, Binding::Awaiting(_)) {
            return Err(TransitionError::NotAwaiting(item));
        }
        entry.binding = Binding::Live(object);
        Ok(vec![Transition {
            item,
            kind: entry.kind,
            change: Change::Bound { object },
            cause: Cause::Spawned,
        }])
    }

    /// The game let go of the item bound to `object`, wherever the table has it: a weapon lies in
    /// the world where it fell, as one the player dropped, and anything else leaves the table. One
    /// already lying in the world stays as it is.
    pub fn dropped(&mut self, object: ObjectAddress) -> Result<Vec<Transition>, TransitionError> {
        let found = self
            .locate(object)
            .ok_or(TransitionError::UnknownObject(object))?;
        if let Place::World(_) = found.place {
            return Ok(Vec::new());
        }
        let (item, from) = self.remove_at(found)?;
        Ok(vec![match Lying::new(item, Departure::Dropped) {
            Ok(lying) => {
                let transition = change(
                    &lying.item,
                    Some(from),
                    Place::World(Departure::Dropped),
                    Cause::Dropped,
                );
                self.world.push(lying);
                transition
            }
            Err(item) => left(&item, from, Cause::Dropped),
        }])
    }

    /// A weapon the mod spawned to stand for one the player has comes onto the body. An item of its
    /// type that waits to be spawned again (a cache's restored weapon whose object is gone) is
    /// bound to it where the table has that item; otherwise it comes onto its socket when the body
    /// has it free and it fits there, or where a weapon goes when the player does not choose.
    pub fn restored(&mut self, restored: Restored) -> Result<Vec<Transition>, TransitionError> {
        if let Some(found) = self.locate(restored.object) {
            return Err(found.already());
        }
        let waiting = self
            .items()
            .filter(|item| item.binding == Binding::Awaiting(Awaiting::Respawn))
            .filter(|item| {
                item.kind.weapon().map(WeaponType::tag_hash) == Some(restored.weapon.tag_hash())
            })
            // The one on the recorded socket first.
            .min_by_key(|item| item.place != Place::Socket(restored.socket))
            .map(|item| item.id);
        if let Some(id) = waiting {
            return self.spawned(id, restored.object);
        }
        let item = Item {
            id: self.allocate(),
            kind: ItemKind::Weapon(restored.weapon),
            binding: Binding::Live(restored.object),
        };
        let socket = restored.socket;
        let item = if self.body.has(socket) && self.sockets[socket.index()].is_none() {
            match Docked::new(item, socket) {
                Ok(docked) => {
                    let transition =
                        change(&docked.item, None, Place::Socket(socket), Cause::Restored);
                    self.sockets[socket.index()] = Some(docked);
                    return Ok(vec![transition]);
                }
                Err(item) => item,
            }
        } else {
            item
        };
        Ok(self
            .onto_body(item, None, Cause::Restored)
            .into_iter()
            .collect())
    }

    /// The game takes items away, from wherever they are, the hands included.
    pub fn remove(
        &mut self,
        target: Removal,
        why: RemovalWhy,
    ) -> Result<Vec<Transition>, TransitionError> {
        let cause = Cause::Removed(why);
        let found = match target {
            Removal::Object(object) => self
                .locate(object)
                .ok_or(TransitionError::UnknownObject(object))?,
            Removal::Item(item) => self
                .locate_item(item)
                .ok_or(TransitionError::UnknownItem(item))?,
            Removal::AllWeapons => {
                return Ok(self.remove_from_body(|item| item.kind.weapon().is_some(), cause));
            }
        };
        let (item, from) = self.remove_at(found)?;
        Ok(vec![left(&item, from, cause)])
    }

    /// Remove every item `matches` picks, from wherever it is.
    pub(super) fn remove_where(
        &mut self,
        matches: impl Fn(&Item) -> bool,
        cause: Cause,
    ) -> Vec<Transition> {
        let mut out = self.remove_from_body(&matches, cause);
        let (gone, kept) = std::mem::take(&mut self.world)
            .into_iter()
            .partition(|lying| matches(&lying.item));
        self.world = kept;
        for lying in gone {
            out.push(left(&lying.item, Place::World(lying.departure), cause));
        }
        out
    }

    /// Remove every item `matches` picks from the hands and the sockets.
    fn remove_from_body(
        &mut self,
        matches: impl Fn(&Item) -> bool,
        cause: Cause,
    ) -> Vec<Transition> {
        let mut out = Vec::new();
        for hand in Hand::BOTH {
            if self.hands[hand.index()].as_ref().is_some_and(&matches)
                && let Some(item) = self.hands[hand.index()].take()
            {
                out.push(left(&item, Place::Hand(hand), cause));
            }
        }
        for socket in Socket::ALL {
            if self.sockets[socket.index()]
                .as_ref()
                .is_some_and(|d| matches(&d.item))
                && let Some(docked) = self.sockets[socket.index()].take()
            {
                out.push(left(&docked.item, Place::Socket(socket), cause));
            }
        }
        out
    }

    /// Put `item` onto the body as a give does: an empty hand (the dominant first), else a free
    /// socket that fits it, else the player's feet.
    pub(super) fn give_item(
        &mut self,
        item: Item,
        from: Option<Place>,
        cause: Cause,
    ) -> Option<Transition> {
        let dominant = self.body.dominant();
        match [dominant, dominant.other()]
            .into_iter()
            .find(|hand| self.hands[hand.index()].is_none())
        {
            Some(hand) => Some(self.put_in_hand(item, hand, from, cause)),
            None => self.onto_body(item, from, cause),
        }
    }

    /// Put `item` on the first free socket that fits it, or at the player's feet with none free.
    /// An item that is no weapon leaves the table, and makes no transition when it was not in it.
    pub(super) fn onto_body(
        &mut self,
        item: Item,
        from: Option<Place>,
        cause: Cause,
    ) -> Option<Transition> {
        let free = item.kind.weapon().and_then(|weapon| {
            self.body
                .preference(weapon)
                .find(|socket| self.sockets[socket.index()].is_none())
        });
        let item = match free {
            Some(socket) => match Docked::new(item, socket) {
                Ok(docked) => {
                    let transition = change(&docked.item, from, Place::Socket(socket), cause);
                    self.sockets[socket.index()] = Some(docked);
                    return Some(transition);
                }
                Err(item) => item,
            },
            None => item,
        };
        self.at_feet(item, from, cause)
    }

    /// Put `item` at the player's feet. An item that cannot lie in the world leaves the table, and
    /// makes no transition when it was not in it.
    fn at_feet(&mut self, item: Item, from: Option<Place>, cause: Cause) -> Option<Transition> {
        match Lying::new(item, Departure::Returned) {
            Ok(lying) => {
                let transition =
                    change(&lying.item, from, Place::World(Departure::Returned), cause);
                self.world.push(lying);
                Some(transition)
            }
            Err(item) => from.map(|from| left(&item, from, cause)),
        }
    }

    /// Put `item` into `hand`, which the caller has checked is empty.
    pub(super) fn put_in_hand(
        &mut self,
        item: Item,
        hand: Hand,
        from: Option<Place>,
        cause: Cause,
    ) -> Transition {
        let transition = change(&item, from, Place::Hand(hand), cause);
        self.hands[hand.index()] = Some(item);
        transition
    }

    pub(super) fn require_empty(&self, hand: Hand) -> Result<(), TransitionError> {
        match &self.hands[hand.index()] {
            Some(item) => Err(TransitionError::HandOccupied {
                hand,
                item: item.id,
            }),
            None => Ok(()),
        }
    }

    fn require_free(&self, socket: Socket) -> Result<(), TransitionError> {
        match &self.sockets[socket.index()] {
            Some(docked) => Err(TransitionError::SocketOccupied {
                socket,
                item: docked.item.id,
            }),
            None => Ok(()),
        }
    }

    /// Where the item bound to `object` is.
    pub(super) fn locate(&self, object: ObjectAddress) -> Option<Found> {
        self.locate_by(|item| item.binding == Binding::Live(object))
    }

    pub(super) fn locate_item(&self, id: ItemId) -> Option<Found> {
        self.locate_by(|item| item.id == id)
    }

    fn locate_by(&self, matches: impl Fn(&Item) -> bool) -> Option<Found> {
        let found = |spot, item: &Item, place| Found {
            spot,
            id: item.id,
            kind: item.kind,
            place,
        };
        for hand in Hand::BOTH {
            if let Some(item) = self.hands[hand.index()].as_ref().filter(|i| matches(i)) {
                return Some(found(Spot::Hand(hand), item, Place::Hand(hand)));
            }
        }
        for socket in Socket::ALL {
            if let Some(docked) = self.sockets[socket.index()]
                .as_ref()
                .filter(|d| matches(&d.item))
            {
                return Some(found(
                    Spot::Socket(socket),
                    &docked.item,
                    Place::Socket(socket),
                ));
            }
        }
        self.world
            .iter()
            .enumerate()
            .find(|(_, lying)| matches(&lying.item))
            .map(|(index, lying)| {
                found(
                    Spot::World(index),
                    &lying.item,
                    Place::World(lying.departure),
                )
            })
    }

    pub(super) fn item_mut(&mut self, id: ItemId) -> Option<&mut Item> {
        let hands = self.hands.iter_mut().flatten();
        let sockets = self.sockets.iter_mut().flatten().map(|d| &mut d.item);
        let world = self.world.iter_mut().map(|l| &mut l.item);
        hands.chain(sockets).chain(world).find(|item| item.id == id)
    }

    /// Take the item [`Loadout::locate`] found out of the table, with where it was.
    pub(super) fn remove_at(&mut self, found: Found) -> Result<(Item, Place), TransitionError> {
        let item = match found.spot {
            Spot::Hand(hand) => self.hands[hand.index()].take(),
            Spot::Socket(socket) => self.sockets[socket.index()].take().map(|d| d.item),
            Spot::World(index) if index < self.world.len() => Some(self.world.remove(index).item),
            Spot::World(_) => None,
        };
        // A spot found and used under one borrow always holds its item; the error keeps a
        // misuse from panicking.
        item.map(|item| (item, found.place))
            .ok_or(TransitionError::UnknownItem(found.id))
    }
}

/// Where an item sits in the table's containers.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Spot {
    Hand(Hand),
    Socket(Socket),
    World(usize),
}

/// An item [`Loadout::locate`] found: where it sits, and what it is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Found {
    pub(super) spot: Spot,
    pub(super) id: ItemId,
    pub(super) kind: ItemKind,
    pub(super) place: Place,
}

impl Found {
    /// The refusal of an object that is already this item.
    fn already(self) -> TransitionError {
        TransitionError::AlreadyInTable {
            item: self.id,
            place: self.place,
        }
    }
}

/// The transition of `item` arriving at `to`, from `from` or from outside the table.
pub(super) fn change(item: &Item, from: Option<Place>, to: Place, cause: Cause) -> Transition {
    Transition {
        item: item.id,
        kind: item.kind,
        change: match from {
            Some(from) => Change::Moved { from, to },
            None => Change::Entered { to },
        },
        cause,
    }
}

pub(super) fn left(item: &Item, from: Place, cause: Cause) -> Transition {
    Transition {
        item: item.id,
        kind: item.kind,
        change: Change::Left { from },
        cause,
    }
}
