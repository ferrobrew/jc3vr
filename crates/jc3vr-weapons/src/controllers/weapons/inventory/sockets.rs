//! The game's side of the loadout's sockets ([`Socket`]): the bone and the offset a weapon the mod
//! places on each sits by, and the name the save sidecar keeps it under. Pure.
//!
//! A slot's weapon on the socket the game docks its slot on keeps the game's dock bone. Any other
//! weapon on a socket is placed by the mod on one of the game's dock bones, with an offset in the
//! character's frame, so a weapon beyond the slots sits beside the slot's weapon on the same bone
//! rather than inside it ([`placed_offset`]).

use glam::Vec3;
use jc3gi::character::character::SafeBoneIndex;
use jc3vr_input::frame::Hand;
use serde::Deserialize;

use crate::controllers::loadout::Socket;

/// The name the save sidecar keeps `socket` under. The first back socket is the right shoulder's,
/// and the second is the left's.
pub const fn key(socket: Socket) -> &'static str {
    match socket {
        Socket::Shoulder(Hand::Right) => "back_first",
        Socket::Shoulder(Hand::Left) => "back_second",
        Socket::Hip(Hand::Left) => "hip_left",
        Socket::Hip(Hand::Right) => "hip_right",
    }
}

/// The socket a sidecar's `key` names.
pub fn from_key(key: &str) -> Option<Socket> {
    Socket::ALL.into_iter().find(|&s| self::key(s) == key)
}

/// The bone a weapon carried on `socket` is placed on: each shoulder's the back dock bone the game
/// docks that side's slot weapon on, and the hips' their holster bones.
pub const fn bone(socket: Socket) -> SafeBoneIndex {
    match socket {
        Socket::Shoulder(Hand::Right) => SafeBoneIndex::BACK_ATTACH,
        Socket::Shoulder(Hand::Left) => SafeBoneIndex::BACK_ATTACH_2,
        Socket::Hip(Hand::Left) => SafeBoneIndex::LEFT_HOLSTER,
        Socket::Hip(Hand::Right) => SafeBoneIndex::RIGHT_HOLSTER,
    }
}

/// The offset a weapon on `socket` is placed by from its bone, in the character's frame: none for a
/// hip, whose weapon sits in the hip's holster, unless the pistol slot holds the game's pair
/// (`pair`), whose two pistols take both holsters; any other socket's configured offset.
pub(crate) fn placed_offset(offsets: &Offsets, socket: Socket, pair: bool) -> Vec3 {
    match socket {
        Socket::Hip(_) if !pair => Vec3::ZERO,
        _ => offsets.of(socket),
    }
}

/// Where each socket's weapon sits relative to its bone, in the character's frame (x across, y up,
/// z backward), metres. The fields keep the save sidecar's socket names ([`key`]), the first back
/// socket being the right shoulder's and the second the left's.
#[derive(Debug, Copy, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offsets {
    pub(crate) back_first: Vec3,
    pub(crate) back_second: Vec3,
    pub(crate) hip_left: Vec3,
    pub(crate) hip_right: Vec3,
}

impl Offsets {
    /// The offset of `socket`, or zero for one that is not finite.
    pub fn of(&self, socket: Socket) -> Vec3 {
        let offset = match socket {
            Socket::Shoulder(Hand::Right) => self.back_first,
            Socket::Shoulder(Hand::Left) => self.back_second,
            Socket::Hip(Hand::Left) => self.hip_left,
            Socket::Hip(Hand::Right) => self.hip_right,
        };
        if offset.is_finite() {
            offset
        } else {
            Vec3::ZERO
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::weapons::inventory::tuning::INVENTORY;

    fn shipped() -> Offsets {
        INVENTORY.get().sockets
    }

    #[test]
    fn the_keys_round_trip_and_keep_their_names() {
        for socket in Socket::ALL {
            assert_eq!(from_key(key(socket)), Some(socket));
        }
        assert_eq!(from_key("back_first"), Some(Socket::Shoulder(Hand::Right)));
        assert_eq!(from_key("back_second"), Some(Socket::Shoulder(Hand::Left)));
        for absent in ["shoulder", "chest", "back_outer", "foot"] {
            assert_eq!(from_key(absent), None);
        }
    }

    #[test]
    fn a_pistol_sits_in_its_hip_s_holster_unless_the_pair_takes_both() {
        let offsets = shipped();
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
            placed_offset(&offsets, Socket::Shoulder(Hand::Left), false),
            offsets.of(Socket::Shoulder(Hand::Left))
        );
    }

    #[test]
    fn a_bad_offset_reads_as_none() {
        let offsets = Offsets {
            back_second: Vec3::NAN,
            ..shipped()
        };
        assert_eq!(offsets.of(Socket::Shoulder(Hand::Left)), Vec3::ZERO);
        assert_eq!(
            offsets.of(Socket::Shoulder(Hand::Right)),
            shipped().back_first
        );
    }
}
