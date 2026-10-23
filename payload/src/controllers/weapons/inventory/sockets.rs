//! The game's side of the loadout's sockets ([`Socket`]): the bone and the offset a weapon the mod
//! places on each sits by, and the name the save sidecar keeps it under. Pure.
//!
//! A slot's weapon on the socket the game docks its slot on keeps the game's dock bone. Any other
//! weapon on a socket is placed by the mod: on one of the game's dock bones (or the chest's
//! explosive bone) with an offset in the character's frame, so a weapon beyond the slots sits
//! beside the slot's weapon on the same bone rather than inside it ([`placed_offset`]).

use glam::Vec3;
use jc3gi::character::character::SafeBoneIndex;

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

/// The offset a weapon on `socket` is placed by from its bone, in the character's frame: none for a
/// hip, whose weapon sits in the hip's holster, unless the pistol slot holds the game's pair
/// (`pair`), whose two pistols take both holsters; any other socket's configured offset.
pub fn placed_offset(offsets: &Offsets, socket: Socket, pair: bool) -> Vec3 {
    match socket {
        Socket::Hip(_) if !pair => Vec3::ZERO,
        _ => offsets.of(socket),
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
    fn a_pistol_sits_in_its_hip_s_holster_unless_the_pair_takes_both() {
        let offsets = Offsets::new();
        for side in Hand::BOTH {
            assert_eq!(
                placed_offset(&offsets, Socket::Hip(side), false),
                Vec3::ZERO
            );
            assert_eq!(
                placed_offset(&offsets, Socket::Hip(side), true),
                offsets.of(Socket::Hip(side))
            );
        }
        assert_eq!(
            placed_offset(&offsets, Socket::Chest, false),
            offsets.of(Socket::Chest)
        );
    }

    #[test]
    fn a_bad_offset_reads_as_none() {
        let mut offsets = Offsets::new();
        offsets.chest = Vec3::NAN;
        assert_eq!(offsets.of(Socket::Chest), Vec3::ZERO);
        assert_eq!(offsets.of(Socket::BackOuter), Vec3::new(0.0, -0.05, 0.14));
    }
}
