//! Where a weapon taken from the world goes, decided against the table's snapshot. Pure.
//!
//! Every take (off the ground, a rack or a locker, out of a character's hands, or by the game's own
//! use-item pickup) lands in the table and then in a hand: the table takes it into the grabbing
//! hand, or for the game's use-item pickup, which no hand grabs, into a free hand, the dominant
//! first. The game's pickup, whose equip puts the wielded weapon away and so takes the other hand's
//! gun, runs only where the table cannot hold the weapon in a hand of its own: while the table does
//! not run or the weapons keep the game's own layout, for a weapon of the heavy slot (which stays
//! the game's), for the embedded power core (whose pickup spawns and equips another weapon), and
//! with a weapon in each hand off while the other hand holds one.

use std::fmt;

use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::{
    frame::Hand,
    loadout::{item::WeaponType, table::LoadoutSnapshot},
};

/// A weapon offered to the hands.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TakeOffer {
    /// The hand that grabbed it, or `None` for the game's use-item pickup.
    pub hand: Option<Hand>,
    pub weapon: WeaponType,
    /// Whether the weapon's pickup gives another weapon in its place (the embedded power core).
    pub swaps: bool,
}

/// What the player allows a take.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TakeRules {
    /// Whether a weapon rides the hand that took it, rather than the game's own layout.
    pub either_hand: bool,
    /// Whether a hand may take a weapon while the other hand holds one.
    pub both_hands: bool,
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
    /// The weapons keep the game's own layout rather than the hand that took them.
    GameLayout,
    /// A weapon of the heavy slot, which stays the game's.
    Heavy,
    /// A pickup that gives another weapon in its place.
    Swaps,
    /// The other hand holds a weapon, and a weapon in each hand is off.
    OneWeapon,
}

impl GameTake {
    pub const fn label(self) -> &'static str {
        match self {
            GameTake::TableStopped => "the table does not run",
            GameTake::GameLayout => "the weapons keep the game's layout",
            GameTake::Heavy => "a heavy weapon stays the game's",
            GameTake::Swaps => "its pickup gives another weapon",
            GameTake::OneWeapon => "a weapon in each hand is off",
        }
    }
}

/// Why a take did not happen.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TakeRefusal {
    /// The grabbing hand holds something already.
    HandFull(Hand),
    /// The game's use-item pickup found no free hand.
    BothHandsFull,
}

impl fmt::Display for TakeRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TakeRefusal::HandFull(hand) => {
                write!(f, "take: the {} hand holds something already", hand.label())
            }
            TakeRefusal::BothHandsFull => write!(f, "take: both hands hold something"),
        }
    }
}

impl std::error::Error for TakeRefusal {}

/// How `offer` is taken, with the table as `snapshot` has it (`None` while it does not run).
pub fn route(
    snapshot: Option<&LoadoutSnapshot>,
    offer: TakeOffer,
    rules: TakeRules,
) -> Result<TakeRoute, TakeRefusal> {
    let Some(snapshot) = snapshot else {
        return Ok(TakeRoute::Game(GameTake::TableStopped));
    };
    if !rules.either_hand {
        return Ok(TakeRoute::Game(GameTake::GameLayout));
    }
    if offer.weapon.slot() == WeaponSlot::E_SLOT_HEAVY {
        return Ok(TakeRoute::Game(GameTake::Heavy));
    }
    if offer.swaps {
        return Ok(TakeRoute::Game(GameTake::Swaps));
    }
    let free = |hand: Hand| snapshot.held(hand).is_none();
    let hand = match offer.hand {
        Some(hand) if free(hand) => hand,
        Some(hand) => return Err(TakeRefusal::HandFull(hand)),
        None => {
            let dominant = snapshot.body.dominant();
            [dominant, dominant.other()]
                .into_iter()
                .find(|&hand| free(hand))
                .ok_or(TakeRefusal::BothHandsFull)?
        }
    };
    let other_weapon = snapshot
        .held(hand.other())
        .is_some_and(|item| item.kind.weapon().is_some());
    if other_weapon && !rules.both_hands {
        return Ok(TakeRoute::Game(GameTake::OneWeapon));
    }
    Ok(TakeRoute::Table { hand })
}
