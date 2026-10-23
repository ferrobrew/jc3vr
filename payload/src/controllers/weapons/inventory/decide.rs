//! What a hand's grab of a socket, and the opening of the second weapon's grip, do. Pure.
//!
//! A hand may hold the wielded weapon, the second weapon (driven by the mod beside the wielded
//! one), or nothing. A socket may hold a carried weapon, or be free.

use crate::controllers::loadout::Socket;

/// What the hands hold, as a grab of a socket sees them.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct HandsView {
    /// The grabbing hand holds the wielded weapon.
    pub holds_wielded: bool,
    /// The grabbing hand holds the second weapon.
    pub holds_second: bool,
    /// The other hand holds the wielded weapon.
    pub other_wields: bool,
    /// The other hand holds the second weapon.
    pub other_holds_second: bool,
    /// Whether a second weapon may be taken.
    pub second_allowed: bool,
}

/// What a grab of a socket does.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GrabDecision {
    /// Draw the carried weapon into the hand as the wielded weapon.
    Draw,
    /// Draw the carried weapon into the hand as the second weapon.
    DrawSecond,
    /// Put the wielded weapon onto the socket.
    Stow,
    /// Put the second weapon onto the socket.
    StowSecond,
    /// The hand holds a weapon and the socket holds another.
    Occupied,
    /// The other hand's weapon leaves no room for another.
    OtherHandFull,
    /// Nothing is carried there and the hand holds nothing to put there.
    Nothing,
}

/// What the grab of a socket that holds a carried weapon when `occupied` does, with the hands as
/// `hands` has them.
pub fn grab(occupied: bool, hands: HandsView) -> GrabDecision {
    if hands.holds_second {
        return if occupied {
            GrabDecision::Occupied
        } else {
            GrabDecision::StowSecond
        };
    }
    match (occupied, hands.holds_wielded) {
        (true, true) => GrabDecision::Occupied,
        (false, true) => GrabDecision::Stow,
        (false, false) => GrabDecision::Nothing,
        (true, false) if hands.other_holds_second => GrabDecision::OtherHandFull,
        (true, false) if hands.other_wields && hands.second_allowed => GrabDecision::DrawSecond,
        (true, false) if hands.other_wields => GrabDecision::OtherHandFull,
        (true, false) => GrabDecision::Draw,
    }
}

/// Where the second weapon's hand was when its grip opened.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ReleasedAt {
    /// A socket's volume.
    Socket(Socket),
    /// A holster that takes the second weapon back: its own slot's holster, or any holster for a
    /// carried weapon.
    OwnHolster,
    /// Anywhere else.
    Elsewhere,
}

/// What opening the second weapon's grip does.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SecondRelease {
    /// Dock it on the socket.
    Dock(Socket),
    /// Put it back where it belongs.
    PutBack,
    /// Let it fall with the hand's motion.
    Fall,
}

/// What opening the second weapon's grip at `at` does.
pub fn second_release(at: ReleasedAt) -> SecondRelease {
    match at {
        ReleasedAt::Socket(socket) => SecondRelease::Dock(socket),
        ReleasedAt::OwnHolster => SecondRelease::PutBack,
        ReleasedAt::Elsewhere => SecondRelease::Fall,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::frame::Hand;

    fn hands() -> HandsView {
        HandsView {
            second_allowed: true,
            ..HandsView::default()
        }
    }

    #[test]
    fn an_empty_hand_draws_a_carried_weapon_into_the_slot_with_the_other_hand_free() {
        assert_eq!(grab(true, hands()), GrabDecision::Draw);
        assert_eq!(grab(false, hands()), GrabDecision::Nothing);
    }

    #[test]
    fn beside_the_wielded_weapon_a_carried_one_comes_as_the_second() {
        let other = HandsView {
            other_wields: true,
            ..hands()
        };
        assert_eq!(grab(true, other), GrabDecision::DrawSecond);
        let no_second = HandsView {
            second_allowed: false,
            ..other
        };
        assert_eq!(grab(true, no_second), GrabDecision::OtherHandFull);
        let other_second = HandsView {
            other_holds_second: true,
            ..hands()
        };
        assert_eq!(grab(true, other_second), GrabDecision::OtherHandFull);
    }

    #[test]
    fn a_full_hand_puts_its_weapon_onto_a_free_socket_only() {
        let wielding = HandsView {
            holds_wielded: true,
            ..hands()
        };
        assert_eq!(grab(false, wielding), GrabDecision::Stow);
        assert_eq!(grab(true, wielding), GrabDecision::Occupied);
        let second = HandsView {
            holds_second: true,
            other_wields: true,
            ..hands()
        };
        assert_eq!(grab(false, second), GrabDecision::StowSecond);
        assert_eq!(grab(true, second), GrabDecision::Occupied);
    }

    #[test]
    fn the_second_weapon_docks_goes_back_or_falls() {
        assert_eq!(
            second_release(ReleasedAt::Socket(Socket::Hip(Hand::Left))),
            SecondRelease::Dock(Socket::Hip(Hand::Left))
        );
        assert_eq!(
            second_release(ReleasedAt::OwnHolster),
            SecondRelease::PutBack
        );
        assert_eq!(second_release(ReleasedAt::Elsewhere), SecondRelease::Fall);
    }
}
