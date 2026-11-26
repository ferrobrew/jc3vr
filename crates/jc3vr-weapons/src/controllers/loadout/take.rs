//! Where a weapon taken from the world goes, decided against the table's snapshot. Pure.
//!
//! Every take (off the ground, a rack or a locker, out of a character's hands, or by the game's own
//! use-item pickup) lands in the table and then in a hand: the table takes it into the grabbing
//! hand, or for the game's use-item pickup, which no hand grabs, into a free hand, the dominant
//! first. The game's pickup, whose equip puts the wielded weapon away and so takes the other hand's
//! gun, runs only where the table cannot hold the weapon in a hand of its own: while the table does
//! not run, and for the embedded power core (whose pickup spawns and equips another weapon).

use jc3vr_body::controllers::held::pickup::TakeRefusal;
use jc3vr_input::frame::Hand;

use crate::controllers::loadout::{item::WeaponType, table::LoadoutSnapshot};

/// A weapon offered to the hands.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TakeOffer {
    /// The hand that grabbed it, or `None` for the game's use-item pickup.
    pub hand: Option<Hand>,
    pub weapon: WeaponType,
    /// Whether the weapon's pickup gives another weapon in its place (the embedded power core).
    pub swaps: bool,
}

/// How a take is carried out.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TakeRoute {
    /// The table takes the weapon into `hand`, and the reconcile wields it there when the hand is
    /// the primary and arms it otherwise. The game's equip never runs, so the other hand keeps
    /// what it holds.
    Table { hand: Hand },
    /// The game's pickup takes it, equipping it and putting the wielded weapon away.
    Game(GameTake),
}

/// Why a take is the game's.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GameTake {
    /// The table does not run.
    TableStopped,
    /// A pickup that gives another weapon in its place.
    Swaps,
}

impl GameTake {
    pub const fn label(self) -> &'static str {
        match self {
            GameTake::TableStopped => "the table does not run",
            GameTake::Swaps => "its pickup gives another weapon",
        }
    }
}

/// How `offer` is taken, with the table as `snapshot` has it (`None` while it does not run).
pub fn route(
    snapshot: Option<&LoadoutSnapshot>,
    offer: TakeOffer,
) -> Result<TakeRoute, TakeRefusal> {
    let Some(snapshot) = snapshot else {
        return Ok(TakeRoute::Game(GameTake::TableStopped));
    };
    if offer.swaps {
        return Ok(TakeRoute::Game(GameTake::Swaps));
    }
    let free = |hand: Hand| snapshot.held(hand).is_none();
    let hand = match offer.hand {
        Some(hand) if free(hand) => hand,
        Some(hand) => return Err(TakeRefusal::HandFull(hand)),
        None => {
            let dominant = snapshot.body.dominant();
            Hand::first_free(dominant, free).ok_or(TakeRefusal::BothHandsFull)?
        }
    };
    Ok(TakeRoute::Table { hand })
}
