//! The carried weapons' side of the loadout's sockets ([`Socket`]): the bone and the offset a
//! weapon carried on each is placed by, the name the save sidecar keeps it under, which sockets the
//! settings give the carry set, and which a carried weapon goes to. Pure.
//!
//! The slots' own weapons keep the game's dock bones. A carried weapon, one beyond the slots,
//! docks on a socket: one of those bones (or the chest's explosive bone) with an offset in the
//! character's frame, so it sits beside the slot's weapon rather than inside it. The back holds
//! long guns, the hips pistols, and the chest a submachine gun, and how many back and hip sockets
//! there are is a setting. The back's sockets fill the right shoulder first, then the left, then
//! the outer place, whatever the dominant hand, as the game's own docks do.

use glam::Vec3;
use jc3gi::{
    character::{character::SafeBoneIndex, inventory::WeaponSlot},
    equipment::weapon::WeaponClass,
};

use crate::controllers::{frame::Hand, loadout::Socket};

/// The stable name the save sidecar keeps `socket` under: the names of the carried weapons' sockets
/// before they were the loadout's, so that existing sidecars load. The first back socket is the
/// right shoulder's and the second the left's.
pub const fn key(socket: Socket) -> &'static str {
    match socket {
        Socket::Shoulder(Hand::Right) => "back_first",
        Socket::Shoulder(Hand::Left) => "back_second",
        Socket::BackOuter => "back_outer",
        Socket::Hip(Hand::Left) => "hip_left",
        Socket::Hip(Hand::Right) => "hip_right",
        Socket::Chest => "chest",
    }
}

/// The socket a sidecar's `key` names.
pub fn from_key(key: &str) -> Option<Socket> {
    Socket::ALL.into_iter().find(|&s| self::key(s) == key)
}

/// The bone a weapon carried on `socket` is placed on: each shoulder's the back dock bone the game
/// docks that side's slot weapon on, the outer back's the right one, the hips' their holster bones,
/// and the chest's the planted explosive's.
pub const fn bone(socket: Socket) -> SafeBoneIndex {
    match socket {
        Socket::Shoulder(Hand::Right) | Socket::BackOuter => SafeBoneIndex::BACK_ATTACH,
        Socket::Shoulder(Hand::Left) => SafeBoneIndex::BACK_ATTACH_2,
        Socket::Hip(Hand::Left) => SafeBoneIndex::LEFT_HOLSTER,
        Socket::Hip(Hand::Right) => SafeBoneIndex::RIGHT_HOLSTER,
        Socket::Chest => SafeBoneIndex::EQUIPPED_EXPLOSIVE,
    }
}

/// What kind of place a weapon docks in.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Fit {
    /// The two-handed and special slots' weapons, on the back.
    LongGun,
    /// The pistol slot's pistols and revolvers, on the hips.
    Pistol,
    /// The pistol slot's submachine guns, on the chest, or a hip when the chest is taken.
    Smg,
}

/// What fits a weapon of `slot` and class `class` (read as the raw value), for the slots a socket
/// takes.
pub fn fit_of(slot: WeaponSlot, class: i32) -> Option<Fit> {
    match slot {
        WeaponSlot::E_SLOT_TWO_HANDED | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL => Some(Fit::LongGun),
        WeaponSlot::E_SLOT_DUAL_WIELD if class == WeaponClass::E_WEAPON_CLASS_SMG as i32 => {
            Some(Fit::Smg)
        }
        WeaponSlot::E_SLOT_DUAL_WIELD => Some(Fit::Pistol),
        _ => None,
    }
}

/// Which sockets exist.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Layout {
    /// How many back sockets, `0..=3`.
    pub back: u8,
    /// How many hip sockets, `0..=2`: one is the dominant hip's.
    pub hips: u8,
    pub chest: bool,
    /// The dominant hand, whose hip takes a pistol first.
    pub dominant: Hand,
}

impl Layout {
    /// Whether `socket` exists in this layout.
    pub fn has(&self, socket: Socket) -> bool {
        match socket {
            Socket::Shoulder(Hand::Right) => self.back >= 1,
            Socket::Shoulder(Hand::Left) => self.back >= 2,
            Socket::BackOuter => self.back >= 3,
            Socket::Hip(side) if side == self.dominant => self.hips >= 1,
            Socket::Hip(_) => self.hips >= 2,
            Socket::Chest => self.chest,
        }
    }

    /// The sockets a weapon of `fit` may dock on, the first choice first.
    pub fn choices(&self, fit: Fit) -> Vec<Socket> {
        let first_hip = Socket::Hip(self.dominant);
        let second_hip = Socket::Hip(self.dominant.other());
        let order: &[Socket] = match fit {
            Fit::LongGun => &[
                Socket::Shoulder(Hand::Right),
                Socket::Shoulder(Hand::Left),
                Socket::BackOuter,
            ],
            Fit::Pistol => &[first_hip, second_hip],
            Fit::Smg => &[Socket::Chest, first_hip, second_hip],
        };
        order.iter().copied().filter(|s| self.has(*s)).collect()
    }

    /// The socket a weapon of `fit` docks on, with `taken` occupied: the first free choice.
    pub fn choose(&self, fit: Fit, taken: &[Socket]) -> Option<Socket> {
        self.choices(fit).into_iter().find(|s| !taken.contains(s))
    }

    /// Whether a weapon of `fit` may dock on `socket`.
    pub fn accepts(&self, socket: Socket, fit: Fit) -> bool {
        self.choices(fit).contains(&socket)
    }
}

/// Where each socket's weapon sits relative to its bone, in the character's frame (x across, y up,
/// z backward), metres. The fields keep the settings file's names, the first back socket being the
/// right shoulder's and the second the left's.
#[derive(Debug, Copy, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Offsets {
    pub back_first: Vec3,
    pub back_second: Vec3,
    pub back_outer: Vec3,
    pub hip_left: Vec3,
    pub hip_right: Vec3,
    pub chest: Vec3,
}

impl Offsets {
    pub const fn new() -> Self {
        Self {
            back_first: Vec3::new(0.0, 0.0, 0.07),
            back_second: Vec3::new(0.0, 0.0, 0.07),
            back_outer: Vec3::new(0.0, -0.05, 0.14),
            hip_left: Vec3::new(0.0, 0.0, -0.09),
            hip_right: Vec3::new(0.0, 0.0, -0.09),
            chest: Vec3::new(0.0, 0.0, -0.04),
        }
    }

    /// The offset of `socket`, or zero for one that is not finite.
    pub fn of(&self, socket: Socket) -> Vec3 {
        let offset = match socket {
            Socket::Shoulder(Hand::Right) => self.back_first,
            Socket::Shoulder(Hand::Left) => self.back_second,
            Socket::BackOuter => self.back_outer,
            Socket::Hip(Hand::Left) => self.hip_left,
            Socket::Hip(Hand::Right) => self.hip_right,
            Socket::Chest => self.chest,
        };
        if offset.is_finite() {
            offset
        } else {
            Vec3::ZERO
        }
    }
}

impl Default for Offsets {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: Layout = Layout {
        back: 3,
        hips: 2,
        chest: true,
        dominant: Hand::Right,
    };

    #[test]
    fn weapons_fit_by_slot_and_class() {
        let smg = WeaponClass::E_WEAPON_CLASS_SMG as i32;
        let pistol = WeaponClass::E_WEAPON_CLASS_PISTOL as i32;
        assert_eq!(
            fit_of(WeaponSlot::E_SLOT_TWO_HANDED, smg),
            Some(Fit::LongGun)
        );
        assert_eq!(
            fit_of(WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL, 0),
            Some(Fit::LongGun)
        );
        assert_eq!(fit_of(WeaponSlot::E_SLOT_DUAL_WIELD, smg), Some(Fit::Smg));
        assert_eq!(
            fit_of(WeaponSlot::E_SLOT_DUAL_WIELD, pistol),
            Some(Fit::Pistol)
        );
        assert_eq!(fit_of(WeaponSlot::E_SLOT_HEAVY, 0), None);
        assert_eq!(fit_of(WeaponSlot::E_SLOT_HAND_GRENADE, 0), None);
    }

    #[test]
    fn long_guns_fill_the_back_in_order_and_then_none_is_free() {
        let mut taken = Vec::new();
        for expected in [
            Socket::Shoulder(Hand::Right),
            Socket::Shoulder(Hand::Left),
            Socket::BackOuter,
        ] {
            let socket = FULL.choose(Fit::LongGun, &taken).unwrap();
            assert_eq!(socket, expected);
            taken.push(socket);
        }
        assert_eq!(FULL.choose(Fit::LongGun, &taken), None);
    }

    #[test]
    fn pistols_take_the_dominant_hip_first() {
        assert_eq!(
            FULL.choose(Fit::Pistol, &[]),
            Some(Socket::Hip(Hand::Right))
        );
        let left = Layout {
            dominant: Hand::Left,
            ..FULL
        };
        assert_eq!(left.choose(Fit::Pistol, &[]), Some(Socket::Hip(Hand::Left)));
        assert_eq!(
            FULL.choose(Fit::Pistol, &[Socket::Hip(Hand::Right)]),
            Some(Socket::Hip(Hand::Left))
        );
        let one_hip = Layout { hips: 1, ..FULL };
        assert_eq!(
            one_hip.choose(Fit::Pistol, &[Socket::Hip(Hand::Right)]),
            None
        );
        assert!(!one_hip.has(Socket::Hip(Hand::Left)));
    }

    #[test]
    fn a_smg_takes_the_chest_then_the_hips() {
        assert_eq!(FULL.choose(Fit::Smg, &[]), Some(Socket::Chest));
        assert_eq!(
            FULL.choose(Fit::Smg, &[Socket::Chest]),
            Some(Socket::Hip(Hand::Right))
        );
        let no_chest = Layout {
            chest: false,
            ..FULL
        };
        assert_eq!(
            no_chest.choose(Fit::Smg, &[]),
            Some(Socket::Hip(Hand::Right))
        );
        assert!(!FULL.accepts(Socket::Chest, Fit::Pistol));
        assert!(FULL.accepts(Socket::Hip(Hand::Left), Fit::Smg));
    }

    #[test]
    fn the_capacity_setting_limits_the_back() {
        let two = Layout { back: 2, ..FULL };
        assert_eq!(two.choices(Fit::LongGun).len(), 2);
        let none = Layout { back: 0, ..FULL };
        assert_eq!(none.choose(Fit::LongGun, &[]), None);
    }

    #[test]
    fn the_keys_round_trip_and_keep_their_names() {
        for socket in Socket::ALL {
            assert_eq!(from_key(key(socket)), Some(socket));
        }
        assert_eq!(from_key("back_first"), Some(Socket::Shoulder(Hand::Right)));
        assert_eq!(from_key("back_second"), Some(Socket::Shoulder(Hand::Left)));
        assert_eq!(from_key("shoulder"), None);
    }

    #[test]
    fn the_back_fills_from_the_right_whatever_the_hand() {
        let left = Layout {
            dominant: Hand::Left,
            back: 1,
            ..FULL
        };
        assert_eq!(
            left.choose(Fit::LongGun, &[]),
            Some(Socket::Shoulder(Hand::Right))
        );
        assert!(!left.has(Socket::Shoulder(Hand::Left)));
    }

    #[test]
    fn a_bad_offset_reads_as_none() {
        let mut offsets = Offsets::new();
        offsets.chest = Vec3::NAN;
        assert_eq!(offsets.of(Socket::Chest), Vec3::ZERO);
        assert_eq!(offsets.of(Socket::BackOuter), Vec3::new(0.0, -0.05, 0.14));
    }
}
