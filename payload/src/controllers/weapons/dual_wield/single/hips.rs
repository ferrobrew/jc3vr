//! Which pistol sits on which hip while the pistols are single. Pure.
//!
//! The game holsters the dual-wield slot's weapon on its hip bones, its first gun on the right. A
//! single pistol in the slot has one gun, so it takes one hip, its home ([`home_for`]); the other
//! hip is a socket for a carried pistol, which then sits in that hip's holster rather than beside
//! it ([`hip_offset`]). Moving a carried pistol into the slot swaps them where they are: the slot's
//! pistol goes onto its own hip's socket and the newcomer's hip becomes the slot's home
//! ([`promote_sides`]), so neither pistol moves on the body. A hand's grab at a hip acts on the
//! pistol that sits there ([`hip_grab`]).

use glam::Vec3;

use crate::controllers::{frame::Hand, weapons::inventory::sockets::SocketId};

/// The hip socket on `side`.
pub const fn hip_socket(side: Hand) -> SocketId {
    match side {
        Hand::Left => SocketId::HipLeft,
        Hand::Right => SocketId::HipRight,
    }
}

/// The side of a hip socket, or `None` for another socket.
pub const fn socket_side(socket: SocketId) -> Option<Hand> {
    match socket {
        SocketId::HipLeft => Some(Hand::Left),
        SocketId::HipRight => Some(Hand::Right),
        _ => None,
    }
}

/// The home of a single pistol coming into the slot, with `carried` whether a carried pistol sits
/// on each hip's socket (by [`Hand::index`]): the right hip, where the game holsters it, unless a
/// carried pistol sits there and the left is free.
pub fn home_for(carried: [bool; 2]) -> Hand {
    if carried[Hand::Right.index()] && !carried[Hand::Left.index()] {
        Hand::Left
    } else {
        Hand::Right
    }
}

/// Where a carried weapon on `socket` sits relative to its bone: in the hip's holster, with no
/// offset, when it is a hip socket the slot's pistol does not take (the slot holds a single pistol
/// whose home is the other hip, or no pistol), and at its `configured` offset otherwise.
/// `slot_home` is the slot's single pistol's home, and `pair` whether the slot holds the game's
/// pair, which takes both hips.
pub fn hip_offset(socket: SocketId, slot_home: Option<Hand>, pair: bool, configured: Vec3) -> Vec3 {
    match socket_side(socket) {
        Some(side) if !pair && slot_home != Some(side) => Vec3::ZERO,
        _ => configured,
    }
}

/// Where the two pistols go when the carried pistol on the `promoted` hip moves into the slot and
/// the slot's single pistol, at home on `occupant_home`, comes out: the occupant onto its own
/// hip's socket when that is free and not the newcomer's, and the newcomer's hip becomes the
/// slot's home; otherwise the two trade hips. Returns the occupant's side and the slot's new home.
pub fn promote_sides(promoted: Hand, occupant_home: Hand, home_socket_free: bool) -> (Hand, Hand) {
    if occupant_home != promoted && home_socket_free {
        (occupant_home, promoted)
    } else {
        (promoted, occupant_home)
    }
}

/// The hands a pair held in the hands goes to when it is split in two, with `drawn` the hands
/// holding one of its pistols (by [`Hand::index`]) and `gun_hands` the hand each of its two guns
/// belongs to: the slot's pistol stays with the first gun's hand when that hand has it drawn, and
/// with the other drawn hand otherwise; with both drawn, the second pistol goes into the other
/// hand, so neither hand is left empty. Returns the slot's pistol's hand and the second pistol's.
pub fn split_hands(drawn: [bool; 2], gun_hands: [Hand; 2]) -> (Option<Hand>, Option<Hand>) {
    let first = gun_hands[0];
    match (drawn[first.index()], drawn[first.other().index()]) {
        (true, true) => (Some(first), Some(first.other())),
        (true, false) => (Some(first), None),
        (false, true) => (Some(first.other()), None),
        (false, false) => (None, None),
    }
}

/// What a hand's grab at a hip does while the pistols are single.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HipGrab {
    /// The hip is the slot's pistol's home: its holster, as for any weapon of one gun.
    Slot,
    /// The hip's socket: draw the carried pistol there, put one back, or dock a pistol there, as
    /// a grab of the socket does.
    Socket(SocketId),
    /// Holster the slot's pistol, held in the grabbing hand, on this hip, which becomes its home.
    Rehome,
    /// Nothing sits here for this hand.
    Nothing,
}

/// A hip grab, as [`hip_grab`] sees it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HipView {
    /// The hip grabbed.
    pub side: Hand,
    /// The slot's single pistol's home, when the slot holds one.
    pub slot_home: Option<Hand>,
    /// Whether the hip has a socket in the inventory's layout.
    pub socket: bool,
    /// Whether a carried pistol sits on the hip's socket, or is kept for it while in a hand.
    pub socket_taken: bool,
    /// Whether the grabbing hand holds the slot's pistol.
    pub holds_slot: bool,
}

/// What a grab at a hip does.
pub fn hip_grab(view: HipView) -> HipGrab {
    if view.slot_home == Some(view.side) {
        return HipGrab::Slot;
    }
    if view.holds_slot && !view.socket_taken {
        return HipGrab::Rehome;
    }
    if view.socket {
        return HipGrab::Socket(hip_socket(view.side));
    }
    HipGrab::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFSET: Vec3 = Vec3::new(0.0, 0.0, -0.09);

    #[test]
    fn the_slot_s_pistol_takes_the_right_hip_unless_a_carried_one_is_there() {
        assert_eq!(home_for([false, false]), Hand::Right);
        assert_eq!(home_for([true, false]), Hand::Right);
        assert_eq!(home_for([false, true]), Hand::Left);
        assert_eq!(home_for([true, true]), Hand::Right);
    }

    #[test]
    fn a_carried_pistol_sits_in_the_holster_the_slot_does_not_take() {
        let left = SocketId::HipLeft;
        let right = SocketId::HipRight;
        assert_eq!(
            hip_offset(left, Some(Hand::Right), false, OFFSET),
            Vec3::ZERO
        );
        assert_eq!(hip_offset(right, Some(Hand::Right), false, OFFSET), OFFSET);
        assert_eq!(hip_offset(right, None, false, OFFSET), Vec3::ZERO);
        // The game's pair takes both hips.
        assert_eq!(hip_offset(left, None, true, OFFSET), OFFSET);
        // Other sockets keep their offsets.
        assert_eq!(hip_offset(SocketId::Chest, None, false, OFFSET), OFFSET);
    }

    #[test]
    fn a_promotion_leaves_both_pistols_where_they_sit() {
        // The twin on the left moves into the slot; the slot's pistol, home on the right, goes
        // onto the right hip's socket, and the slot's home is now the left.
        assert_eq!(
            promote_sides(Hand::Left, Hand::Right, true),
            (Hand::Right, Hand::Left)
        );
        // With its own hip's socket taken, the two trade.
        assert_eq!(
            promote_sides(Hand::Left, Hand::Right, false),
            (Hand::Left, Hand::Right)
        );
        // A newcomer from the slot's own side trades too.
        assert_eq!(
            promote_sides(Hand::Right, Hand::Right, true),
            (Hand::Right, Hand::Right)
        );
    }

    #[test]
    fn a_pair_split_in_the_hands_leaves_no_hand_empty() {
        let right_first = [Hand::Right, Hand::Left];
        assert_eq!(
            split_hands([true, true], right_first),
            (Some(Hand::Right), Some(Hand::Left))
        );
        assert_eq!(
            split_hands([false, true], right_first),
            (Some(Hand::Right), None)
        );
        assert_eq!(
            split_hands([true, false], right_first),
            (Some(Hand::Left), None),
            "the slot's pistol stays in the hand that holds a pistol"
        );
        assert_eq!(split_hands([false, false], right_first), (None, None));
    }

    fn view(side: Hand) -> HipView {
        HipView {
            side,
            slot_home: Some(Hand::Right),
            socket: true,
            socket_taken: true,
            holds_slot: false,
        }
    }

    #[test]
    fn a_hip_grab_acts_on_the_pistol_that_sits_there() {
        assert_eq!(hip_grab(view(Hand::Right)), HipGrab::Slot);
        assert_eq!(
            hip_grab(view(Hand::Left)),
            HipGrab::Socket(SocketId::HipLeft)
        );
        let empty_left = HipView {
            socket_taken: false,
            ..view(Hand::Left)
        };
        assert_eq!(
            hip_grab(empty_left),
            HipGrab::Socket(SocketId::HipLeft),
            "a free socket takes what the hand holds"
        );
        let no_socket = HipView {
            socket: false,
            socket_taken: false,
            ..view(Hand::Left)
        };
        assert_eq!(hip_grab(no_socket), HipGrab::Nothing);
    }

    #[test]
    fn the_slot_s_pistol_holstered_at_the_free_hip_moves_home() {
        let rehome = HipView {
            socket_taken: false,
            holds_slot: true,
            ..view(Hand::Left)
        };
        assert_eq!(hip_grab(rehome), HipGrab::Rehome);
        let no_socket = HipView {
            socket: false,
            ..rehome
        };
        assert_eq!(hip_grab(no_socket), HipGrab::Rehome);
        let taken = HipView {
            socket_taken: true,
            ..rehome
        };
        assert_eq!(
            hip_grab(taken),
            HipGrab::Socket(SocketId::HipLeft),
            "a hip with a pistol on it is that pistol's"
        );
        let at_home = HipView {
            holds_slot: true,
            ..view(Hand::Right)
        };
        assert_eq!(hip_grab(at_home), HipGrab::Slot);
    }
}
