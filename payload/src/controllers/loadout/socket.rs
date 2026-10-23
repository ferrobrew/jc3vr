//! The body's sockets: the places on Rico's body a weapon docks, which there are, which weapons
//! each takes, and which a weapon goes to when the player does not choose. Pure.
//!
//! One notion covers what were the holster sites and the carried weapons' sockets: the two hips,
//! the two shoulders (where the game docks its two-handed and special slots' weapons), a third
//! place across the back, and the chest. Each takes items of its size class or smaller
//! ([`Socket::class`]), and which of the hips and shoulders exist follows the dominant hand, so the
//! layout mirrors for handedness ([`BodyLayout`]).

use std::fmt;

use jc3gi::character::inventory::WeaponSlot;

use crate::controllers::{
    frame::Hand,
    loadout::item::{SizeClass, WeaponType},
};

/// A socket on the body.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Socket {
    /// The hip on this side.
    Hip(Hand),
    /// The shoulder on this side, where the game's long guns dock across the back.
    Shoulder(Hand),
    /// Across the back outside both shoulders, for a third long gun.
    BackOuter,
    Chest,
}

impl Socket {
    /// How many sockets there are.
    pub const COUNT: usize = 6;

    /// Every socket, in [`Socket::index`] order.
    pub const ALL: [Socket; Socket::COUNT] = [
        Socket::Hip(Hand::Left),
        Socket::Hip(Hand::Right),
        Socket::Shoulder(Hand::Left),
        Socket::Shoulder(Hand::Right),
        Socket::BackOuter,
        Socket::Chest,
    ];

    /// The socket's index in the per-socket arrays.
    pub const fn index(self) -> usize {
        match self {
            Socket::Hip(hand) => hand.index(),
            Socket::Shoulder(hand) => 2 + hand.index(),
            Socket::BackOuter => 4,
            Socket::Chest => 5,
        }
    }

    /// The largest items the socket takes.
    pub const fn class(self) -> SizeClass {
        match self {
            Socket::Hip(_) | Socket::Chest => SizeClass::Smg,
            Socket::Shoulder(_) | Socket::BackOuter => SizeClass::Heavy,
        }
    }

    /// Whether the socket takes an item of `size`: its own class or smaller.
    pub fn fits(self, size: SizeClass) -> bool {
        size <= self.class()
    }

    /// The game slot whose weapon docks here by the game's own placement, with `dominant` the
    /// dominant hand: the dominant shoulder carries the two-handed slot, the other shoulder the
    /// special slot, and the hips the pistols.
    pub fn native_slot(self, dominant: Hand) -> Option<WeaponSlot> {
        match self {
            Socket::Hip(_) => Some(WeaponSlot::E_SLOT_DUAL_WIELD),
            Socket::Shoulder(side) if side == dominant => Some(WeaponSlot::E_SLOT_TWO_HANDED),
            Socket::Shoulder(_) => Some(WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL),
            Socket::BackOuter | Socket::Chest => None,
        }
    }
}

impl fmt::Display for Socket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Socket::Hip(hand) => write!(f, "the {} hip", hand.label()),
            Socket::Shoulder(hand) => write!(f, "the {} shoulder", hand.label()),
            Socket::BackOuter => f.write_str("the back (outer)"),
            Socket::Chest => f.write_str("the chest"),
        }
    }
}

/// Which sockets the body has, and the dominant hand they mirror for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct BodyLayout {
    dominant: Hand,
    /// How many places on the back, `0..=3`: the dominant shoulder, the other, then the outer.
    back: u8,
    /// How many hips, `0..=2`: the dominant hip, then the other.
    hips: u8,
    chest: bool,
}

impl BodyLayout {
    /// Every socket, for a player of `dominant` handedness.
    pub const fn full(dominant: Hand) -> BodyLayout {
        BodyLayout {
            dominant,
            back: 3,
            hips: 2,
            chest: true,
        }
    }

    /// A layout of `back` places on the back and `hips` hips, each clamped to what the body has.
    pub fn new(dominant: Hand, back: u8, hips: u8, chest: bool) -> BodyLayout {
        BodyLayout {
            dominant,
            back: back.min(3),
            hips: hips.min(2),
            chest,
        }
    }

    pub const fn dominant(self) -> Hand {
        self.dominant
    }

    /// Whether the body has `socket`.
    pub fn has(self, socket: Socket) -> bool {
        match socket {
            Socket::Shoulder(side) if side == self.dominant => self.back >= 1,
            Socket::Shoulder(_) => self.back >= 2,
            Socket::BackOuter => self.back >= 3,
            Socket::Hip(side) if side == self.dominant => self.hips >= 1,
            Socket::Hip(_) => self.hips >= 2,
            Socket::Chest => self.chest,
        }
    }

    /// The sockets a weapon of `weapon`'s type goes to when the player does not choose (a game's
    /// give with the hands full, a lost weapon's return, a respawn), the first choice first: the
    /// socket the game docks its slot on, then the snuggest that fits, the dominant side first.
    pub fn preference(self, weapon: WeaponType) -> impl Iterator<Item = Socket> {
        let dominant = self.dominant;
        let other = dominant.other();
        let by_size: [Socket; Socket::COUNT] = match weapon.size() {
            SizeClass::Pistol => [
                Socket::Hip(dominant),
                Socket::Hip(other),
                Socket::Chest,
                Socket::Shoulder(dominant),
                Socket::Shoulder(other),
                Socket::BackOuter,
            ],
            SizeClass::Smg => [
                Socket::Chest,
                Socket::Hip(dominant),
                Socket::Hip(other),
                Socket::Shoulder(dominant),
                Socket::Shoulder(other),
                Socket::BackOuter,
            ],
            SizeClass::LongGun | SizeClass::Heavy => [
                Socket::Shoulder(dominant),
                Socket::Shoulder(other),
                Socket::BackOuter,
                Socket::Hip(dominant),
                Socket::Hip(other),
                Socket::Chest,
            ],
        };
        let native = Socket::ALL
            .into_iter()
            .filter(move |s| s.native_slot(dominant) == Some(weapon.slot()))
            .min_by_key(|s| by_size.iter().position(|b| b == s));
        native
            .into_iter()
            .chain(by_size.into_iter().filter(move |s| Some(*s) != native))
            .filter(move |s| self.has(*s) && s.fits(weapon.size()))
    }
}
