//! The body's sockets: the places on Rico's body a weapon docks, which there are, which weapons
//! each takes, and which a weapon goes to when the player does not choose. Pure.
//!
//! One notion covers the holster sites and the carried weapons' sockets: the two hips, the two
//! shoulders (where the game docks its two-handed slot's weapon, on the right, and its special
//! slot's, on the left). Each takes items of its size class or smaller
//! ([`Socket::class`]), and which of the hips and shoulders exist follows the dominant hand, so the
//! layout mirrors for handedness ([`BodyLayout`]).

use std::fmt;

use jc3gi::character::inventory::WeaponSlot;
use jc3vr_input::frame::Hand;

use crate::controllers::loadout::item::{SizeClass, WeaponType};

/// A socket on the body.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Socket {
    /// The hip on this side.
    Hip(Hand),
    /// The shoulder on this side, where the game's long guns dock across the back: the two-handed
    /// and heavy slots' on the right, the special slot's on the left.
    Shoulder(Hand),
}

impl Socket {
    /// How many sockets there are.
    pub const COUNT: usize = 4;

    /// Every socket, in [`Socket::index`] order.
    pub const ALL: [Socket; Socket::COUNT] = [
        Socket::Hip(Hand::Left),
        Socket::Hip(Hand::Right),
        Socket::Shoulder(Hand::Left),
        Socket::Shoulder(Hand::Right),
    ];

    /// The socket's index in the per-socket arrays.
    pub const fn index(self) -> usize {
        match self {
            Socket::Hip(hand) => hand.index(),
            Socket::Shoulder(hand) => 2 + hand.index(),
        }
    }

    /// The largest items the socket takes.
    pub const fn class(self) -> SizeClass {
        match self {
            Socket::Hip(_) => SizeClass::Smg,
            Socket::Shoulder(_) => SizeClass::Heavy,
        }
    }

    /// Whether the socket takes an item of `size`: its own class or smaller.
    pub fn fits(self, size: SizeClass) -> bool {
        size <= self.class()
    }

    /// Whether the game docks a weapon of `slot` here by its own placement. The game's choice is
    /// per slot and never mirrored for handedness: the two-handed and heavy slots' weapons dock on
    /// the right of the back, the special slot's on the left, and the pistols on the hips.
    pub fn is_native(self, slot: WeaponSlot) -> bool {
        match self {
            Socket::Hip(_) => slot == WeaponSlot::E_SLOT_DUAL_WIELD,
            Socket::Shoulder(Hand::Right) => matches!(
                slot,
                WeaponSlot::E_SLOT_TWO_HANDED | WeaponSlot::E_SLOT_HEAVY
            ),
            Socket::Shoulder(Hand::Left) => slot == WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
        }
    }

    /// The socket's name as the HUD shows it, in sentence case. The shoulder sockets dock across
    /// the back, so they read as the back's sides.
    pub const fn display_name(self) -> &'static str {
        match self {
            Socket::Hip(Hand::Left) => "Left hip",
            Socket::Hip(Hand::Right) => "Right hip",
            Socket::Shoulder(Hand::Left) => "Left back",
            Socket::Shoulder(Hand::Right) => "Right back",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Socket::Hip(Hand::Left) => "left hip",
            Socket::Hip(Hand::Right) => "right hip",
            Socket::Shoulder(Hand::Left) => "left shoulder",
            Socket::Shoulder(Hand::Right) => "right shoulder",
        }
    }
}

impl fmt::Display for Socket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the {}", self.label())
    }
}

/// Which sockets the body has, and the dominant hand they mirror for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct BodyLayout {
    dominant: Hand,
    /// How many shoulders, `0..=2`: the dominant shoulder, then the other.
    back: u8,
    /// How many hips, `0..=2`: the dominant hip, then the other.
    hips: u8,
}

impl BodyLayout {
    /// Every socket, for a player of `dominant` handedness.
    pub const fn full(dominant: Hand) -> BodyLayout {
        BodyLayout {
            dominant,
            back: 2,
            hips: 2,
        }
    }

    /// A layout of `back` places on the back and `hips` hips, each clamped to what the body has.
    #[cfg(test)]
    pub fn new(dominant: Hand, back: u8, hips: u8) -> BodyLayout {
        BodyLayout {
            dominant,
            back: back.min(2),
            hips: hips.min(2),
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
            Socket::Hip(side) if side == self.dominant => self.hips >= 1,
            Socket::Hip(_) => self.hips >= 2,
        }
    }

    /// The sockets a weapon of `weapon`'s type goes to when the player does not choose (a game's
    /// give with the hands full, a lost weapon's return, a respawn), the first choice first: the
    /// socket the game docks its slot on, then the snuggest that fits, the dominant side first.
    pub fn preference(self, weapon: WeaponType) -> impl Iterator<Item = Socket> {
        let dominant = self.dominant;
        let other = dominant.other();
        let by_size: [Socket; Socket::COUNT] = match weapon.size() {
            SizeClass::Pistol | SizeClass::Smg => [
                Socket::Hip(dominant),
                Socket::Hip(other),
                Socket::Shoulder(dominant),
                Socket::Shoulder(other),
            ],
            SizeClass::LongGun | SizeClass::Heavy => [
                Socket::Shoulder(dominant),
                Socket::Shoulder(other),
                Socket::Hip(dominant),
                Socket::Hip(other),
            ],
        };
        let native = Socket::ALL
            .into_iter()
            .filter(move |s| s.is_native(weapon.slot()))
            .min_by_key(|s| by_size.iter().position(|b| b == s));
        native
            .into_iter()
            .chain(by_size.into_iter().filter(move |s| Some(*s) != native))
            .filter(move |s| self.has(*s) && s.fits(weapon.size()))
    }
}
