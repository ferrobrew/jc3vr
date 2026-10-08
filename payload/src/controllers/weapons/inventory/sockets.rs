//! The places on the body a carried weapon docks: which there are, which weapons each takes, and
//! which a weapon goes to. Pure.
//!
//! The slots' own weapons keep the game's dock bones. A carried weapon, one beyond the slots,
//! docks on a socket: one of those bones (or the chest's explosive bone) with an offset in the
//! character's frame, so it sits beside the slot's weapon rather than inside it. The back holds
//! long guns, the hips pistols, and the chest a submachine gun, and how many back and hip sockets
//! there are is a setting.

use glam::Vec3;
use jc3gi::{
    character::{character::SafeBoneIndex, inventory::WeaponSlot},
    equipment::weapon::WeaponClass,
};

use crate::controllers::frame::Hand;

/// A socket on the body.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SocketId {
    /// The back, beside the first back dock bone's weapon.
    BackFirst,
    /// The back, beside the second back dock bone's weapon.
    BackSecond,
    /// The back, outside both, for a third long gun.
    BackOuter,
    HipLeft,
    HipRight,
    Chest,
}

impl SocketId {
    pub const ALL: [SocketId; 6] = [
        SocketId::BackFirst,
        SocketId::BackSecond,
        SocketId::BackOuter,
        SocketId::HipLeft,
        SocketId::HipRight,
        SocketId::Chest,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            SocketId::BackFirst => "back (first)",
            SocketId::BackSecond => "back (second)",
            SocketId::BackOuter => "back (outer)",
            SocketId::HipLeft => "left hip",
            SocketId::HipRight => "right hip",
            SocketId::Chest => "chest",
        }
    }

    /// The stable name the save sidecar uses.
    pub const fn key(self) -> &'static str {
        match self {
            SocketId::BackFirst => "back_first",
            SocketId::BackSecond => "back_second",
            SocketId::BackOuter => "back_outer",
            SocketId::HipLeft => "hip_left",
            SocketId::HipRight => "hip_right",
            SocketId::Chest => "chest",
        }
    }

    /// The socket a sidecar's `key` names.
    pub fn from_key(key: &str) -> Option<SocketId> {
        SocketId::ALL.into_iter().find(|s| s.key() == key)
    }

    /// The bone the socket's weapon is placed on.
    pub const fn bone(self) -> SafeBoneIndex {
        match self {
            SocketId::BackFirst | SocketId::BackOuter => SafeBoneIndex::BACK_ATTACH,
            SocketId::BackSecond => SafeBoneIndex::BACK_ATTACH_2,
            SocketId::HipLeft => SafeBoneIndex::LEFT_HOLSTER,
            SocketId::HipRight => SafeBoneIndex::RIGHT_HOLSTER,
            SocketId::Chest => SafeBoneIndex::EQUIPPED_EXPLOSIVE,
        }
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
    pub fn has(&self, socket: SocketId) -> bool {
        let dominant_hip = match self.dominant {
            Hand::Left => SocketId::HipLeft,
            Hand::Right => SocketId::HipRight,
        };
        match socket {
            SocketId::BackFirst => self.back >= 1,
            SocketId::BackSecond => self.back >= 2,
            SocketId::BackOuter => self.back >= 3,
            SocketId::HipLeft | SocketId::HipRight if socket == dominant_hip => self.hips >= 1,
            SocketId::HipLeft | SocketId::HipRight => self.hips >= 2,
            SocketId::Chest => self.chest,
        }
    }

    /// The sockets a weapon of `fit` may dock on, the first choice first.
    pub fn choices(&self, fit: Fit) -> Vec<SocketId> {
        let (first_hip, second_hip) = match self.dominant {
            Hand::Left => (SocketId::HipLeft, SocketId::HipRight),
            Hand::Right => (SocketId::HipRight, SocketId::HipLeft),
        };
        let order: &[SocketId] = match fit {
            Fit::LongGun => &[
                SocketId::BackFirst,
                SocketId::BackSecond,
                SocketId::BackOuter,
            ],
            Fit::Pistol => &[first_hip, second_hip],
            Fit::Smg => &[SocketId::Chest, first_hip, second_hip],
        };
        order.iter().copied().filter(|s| self.has(*s)).collect()
    }

    /// The socket a weapon of `fit` docks on, with `taken` occupied: the first free choice.
    pub fn choose(&self, fit: Fit, taken: &[SocketId]) -> Option<SocketId> {
        self.choices(fit).into_iter().find(|s| !taken.contains(s))
    }

    /// Whether a weapon of `fit` may dock on `socket`.
    pub fn accepts(&self, socket: SocketId, fit: Fit) -> bool {
        self.choices(fit).contains(&socket)
    }
}

/// Where each socket's weapon sits relative to its bone, in the character's frame (x across, y up,
/// z backward), metres.
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
    pub fn of(&self, socket: SocketId) -> Vec3 {
        let offset = match socket {
            SocketId::BackFirst => self.back_first,
            SocketId::BackSecond => self.back_second,
            SocketId::BackOuter => self.back_outer,
            SocketId::HipLeft => self.hip_left,
            SocketId::HipRight => self.hip_right,
            SocketId::Chest => self.chest,
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
            SocketId::BackFirst,
            SocketId::BackSecond,
            SocketId::BackOuter,
        ] {
            let socket = FULL.choose(Fit::LongGun, &taken).unwrap();
            assert_eq!(socket, expected);
            taken.push(socket);
        }
        assert_eq!(FULL.choose(Fit::LongGun, &taken), None);
    }

    #[test]
    fn pistols_take_the_dominant_hip_first() {
        assert_eq!(FULL.choose(Fit::Pistol, &[]), Some(SocketId::HipRight));
        let left = Layout {
            dominant: Hand::Left,
            ..FULL
        };
        assert_eq!(left.choose(Fit::Pistol, &[]), Some(SocketId::HipLeft));
        assert_eq!(
            FULL.choose(Fit::Pistol, &[SocketId::HipRight]),
            Some(SocketId::HipLeft)
        );
        let one_hip = Layout { hips: 1, ..FULL };
        assert_eq!(one_hip.choose(Fit::Pistol, &[SocketId::HipRight]), None);
        assert!(!one_hip.has(SocketId::HipLeft));
    }

    #[test]
    fn a_smg_takes_the_chest_then_the_hips() {
        assert_eq!(FULL.choose(Fit::Smg, &[]), Some(SocketId::Chest));
        assert_eq!(
            FULL.choose(Fit::Smg, &[SocketId::Chest]),
            Some(SocketId::HipRight)
        );
        let no_chest = Layout {
            chest: false,
            ..FULL
        };
        assert_eq!(no_chest.choose(Fit::Smg, &[]), Some(SocketId::HipRight));
        assert!(!FULL.accepts(SocketId::Chest, Fit::Pistol));
        assert!(FULL.accepts(SocketId::HipLeft, Fit::Smg));
    }

    #[test]
    fn the_capacity_setting_limits_the_back() {
        let two = Layout { back: 2, ..FULL };
        assert_eq!(two.choices(Fit::LongGun).len(), 2);
        let none = Layout { back: 0, ..FULL };
        assert_eq!(none.choose(Fit::LongGun, &[]), None);
    }

    #[test]
    fn the_keys_round_trip() {
        for socket in SocketId::ALL {
            assert_eq!(SocketId::from_key(socket.key()), Some(socket));
        }
        assert_eq!(SocketId::from_key("shoulder"), None);
    }

    #[test]
    fn a_bad_offset_reads_as_none() {
        let mut offsets = Offsets::new();
        offsets.chest = Vec3::NAN;
        assert_eq!(offsets.of(SocketId::Chest), Vec3::ZERO);
        assert_eq!(offsets.of(SocketId::BackOuter), Vec3::new(0.0, -0.05, 0.14));
    }
}
