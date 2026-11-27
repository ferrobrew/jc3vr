//! The body's sockets as the grip model names them: the places on Rico's body a weapon docks, and
//! the size classes each takes. Which sockets the body has and which a weapon goes to are the
//! loadout's (the weapons crate's `controllers::loadout`). Pure.
//!
//! One notion covers the holster sites and the carried weapons' sockets: the two hips, the two
//! shoulders (where the game docks its two-handed slot's weapon, on the right, and its special
//! slot's, on the left). Each takes items of its size class or smaller ([`Socket::class`]).

use std::fmt;

use jc3gi::character::inventory::WeaponSlot;
use jc3vr_input::frame::Hand;

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

/// How big a weapon is, for the body's sockets: a socket takes items of its own class or smaller,
/// in this order.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SizeClass {
    /// The pistols and revolvers.
    Pistol,
    /// The submachine guns.
    Smg,
    /// The two-handed and special weapons.
    LongGun,
    /// The heavy weapons, which go on the back.
    Heavy,
}

impl SizeClass {
    pub const fn label(self) -> &'static str {
        match self {
            SizeClass::Pistol => "pistol",
            SizeClass::Smg => "submachine gun",
            SizeClass::LongGun => "long gun",
            SizeClass::Heavy => "heavy weapon",
        }
    }
}
