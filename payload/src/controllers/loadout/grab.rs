//! What a hand does at a socket of the body, decided against the table's snapshot. Pure.
//!
//! One decision covers every socket: a grab by an empty hand draws the item on the socket, a grab or
//! a release of a held weapon at a free socket that fits it docks it there, and a release anywhere
//! else lets it go. Nothing here selects a slot or asks the game to holster: the table takes the
//! move, and the reconcile makes the game follow.

use std::fmt;

use crate::controllers::{
    frame::Hand,
    loadout::{item::WeaponType, socket::Socket, table::LoadoutSnapshot},
};

/// A move a hand makes at a socket.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SocketMove {
    /// The empty hand takes the item on the socket.
    Draw { socket: Socket, hand: Hand },
    /// The hand's weapon goes onto the socket.
    Dock { hand: Hand, socket: Socket },
}

/// What the player allows a grab at a socket.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct GrabRules {
    /// Whether a hand may draw a weapon while the other hand holds one.
    pub both_hands: bool,
}

/// Why a grab at a socket did nothing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SocketRefusal {
    /// Nothing is on the socket, and the hand holds nothing to put there.
    Empty(Socket),
    /// The hand holds something, and the socket holds another item.
    Occupied(Socket),
    /// What the hand holds does not fit the socket, or the body has no such socket.
    DoesNotFit(Socket),
    /// The other hand holds a weapon, and a weapon in each hand is off.
    OtherHandFull(Socket),
}

impl fmt::Display for SocketRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocketRefusal::Empty(socket) => {
                write!(
                    f,
                    "socket: nothing is on {socket}, and the hand holds nothing"
                )
            }
            SocketRefusal::Occupied(socket) => {
                write!(f, "socket: {socket} holds another item")
            }
            SocketRefusal::DoesNotFit(socket) => {
                write!(f, "socket: what the hand holds does not fit {socket}")
            }
            SocketRefusal::OtherHandFull(socket) => write!(
                f,
                "socket: nothing drawn off {socket}, since the other hand holds a weapon"
            ),
        }
    }
}

impl std::error::Error for SocketRefusal {}

/// What `hand`'s grab at `socket` does, with the table as `snapshot` has it.
pub fn grab(
    snapshot: &LoadoutSnapshot,
    hand: Hand,
    socket: Socket,
    rules: GrabRules,
) -> Result<SocketMove, SocketRefusal> {
    let held = snapshot.held(hand).is_some();
    let docked = snapshot.docked(socket).is_some();
    match (held, docked) {
        (false, true) if !rules.both_hands && holds_weapon(snapshot, hand.other()) => {
            Err(SocketRefusal::OtherHandFull(socket))
        }
        (false, true) => Ok(SocketMove::Draw { socket, hand }),
        (false, false) => Err(SocketRefusal::Empty(socket)),
        (true, true) => Err(SocketRefusal::Occupied(socket)),
        (true, false) if fits(snapshot, hand, socket) => Ok(SocketMove::Dock { hand, socket }),
        (true, false) => Err(SocketRefusal::DoesNotFit(socket)),
    }
}

/// Whether `socket` takes the weapon `hand` holds: the body has it, it is free, and the weapon is
/// of its size class or smaller.
pub fn fits(snapshot: &LoadoutSnapshot, hand: Hand, socket: Socket) -> bool {
    let Some(weapon) = held_weapon(snapshot, hand) else {
        return false;
    };
    snapshot.body.has(socket) && socket.fits(weapon.size()) && snapshot.docked(socket).is_none()
}

/// The sockets of the body that do not take the weapon `hand` holds: taken, or too small for it.
/// Empty while the hand holds no weapon.
pub fn refusing(snapshot: &LoadoutSnapshot, hand: Hand) -> impl Iterator<Item = Socket> + '_ {
    let holds = holds_weapon(snapshot, hand);
    Socket::ALL
        .into_iter()
        .filter(move |&socket| holds && snapshot.body.has(socket) && !fits(snapshot, hand, socket))
}

/// What opening a hand's grip on its weapon does.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Release {
    /// The weapon docks on this socket.
    Dock(Socket),
    /// The weapon leaves the hand into the world.
    LetGo,
    /// The weapon stays in the hand: it cannot lie in the world, and no socket takes it.
    Keep,
}

/// What opening `hand`'s grip does, over `over` (the nearest socket under the hand that takes the
/// weapon, if any), for a weapon that can lie in the world when `droppable`. A weapon that cannot
/// (the game's dual-wield pair, one object of two guns) goes onto the first free socket the body
/// prefers for it wherever the hand opens, as the game's pair goes back to its hips.
pub fn release(
    snapshot: &LoadoutSnapshot,
    hand: Hand,
    over: Option<Socket>,
    droppable: bool,
) -> Release {
    if let Some(socket) = over.filter(|&socket| fits(snapshot, hand, socket)) {
        return Release::Dock(socket);
    }
    if droppable {
        return Release::LetGo;
    }
    let Some(weapon) = held_weapon(snapshot, hand) else {
        return Release::LetGo;
    };
    snapshot
        .body
        .preference(weapon)
        .find(|&socket| fits(snapshot, hand, socket))
        .map_or(Release::Keep, Release::Dock)
}

/// The type of the weapon `hand` holds in `snapshot`.
fn held_weapon(snapshot: &LoadoutSnapshot, hand: Hand) -> Option<WeaponType> {
    snapshot.held(hand).and_then(|item| item.kind.weapon())
}

fn holds_weapon(snapshot: &LoadoutSnapshot, hand: Hand) -> bool {
    held_weapon(snapshot, hand).is_some()
}
