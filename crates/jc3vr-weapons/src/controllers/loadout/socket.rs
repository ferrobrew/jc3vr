//! The body's sockets: which of them the body has, and which a weapon goes to when the player does
//! not choose. Pure.
//!
//! The sockets themselves and the size classes they take are the grip model's ([`Socket`]). Which
//! of the hips and shoulders exist follows the dominant hand, so the layout mirrors for handedness
//! ([`BodyLayout`]).

use jc3vr_body::controllers::grab::socket::{SizeClass, Socket};
use jc3vr_input::frame::Hand;

use crate::controllers::loadout::item::WeaponType;

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
    pub(crate) fn preference(self, weapon: WeaponType) -> impl Iterator<Item = Socket> {
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
