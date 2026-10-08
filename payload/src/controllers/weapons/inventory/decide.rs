//! What a hand's grab of a socket, and the opening of the second weapon's grip, do. Pure.
//!
//! A hand may hold the wielded weapon, the second weapon (driven by the mod beside the wielded
//! one), or nothing. A socket may hold a carried weapon, or be free.

use crate::controllers::weapons::inventory::sockets::SocketId;

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
    Socket(SocketId),
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
    Dock(SocketId),
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

/// Watches for the second weapon held alone, which it then becomes the wielded weapon after a
/// while.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AloneWatch {
    alone_s: Option<f32>,
}

impl AloneWatch {
    pub const fn new() -> Self {
        Self { alone_s: None }
    }

    /// Fold in one tick of `dt` seconds: `second` whether a second weapon is held, and `wielded`
    /// whether any other weapon is wielded. Returns whether the second weapon has been held alone
    /// for `after_s`, once, after which the watch starts over.
    pub fn observe(&mut self, second: bool, wielded: bool, dt: f32, after_s: f32) -> bool {
        if !second || wielded {
            self.alone_s = None;
            return false;
        }
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let alone = self.alone_s.map_or(0.0, |s| s + dt);
        let after = if after_s.is_finite() {
            after_s.max(0.0)
        } else {
            0.0
        };
        if alone >= after {
            self.alone_s = None;
            return true;
        }
        self.alone_s = Some(alone);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            second_release(ReleasedAt::Socket(SocketId::HipLeft)),
            SecondRelease::Dock(SocketId::HipLeft)
        );
        assert_eq!(
            second_release(ReleasedAt::OwnHolster),
            SecondRelease::PutBack
        );
        assert_eq!(second_release(ReleasedAt::Elsewhere), SecondRelease::Fall);
    }

    #[test]
    fn a_lone_second_weapon_is_promoted_after_a_while_once() {
        let mut w = AloneWatch::new();
        let tick = 0.1;
        assert!(
            !w.observe(true, true, tick, 0.45),
            "the wielded weapon is there"
        );
        let mut ticks = 0;
        while !w.observe(true, false, tick, 0.45) {
            ticks += 1;
            assert!(ticks < 10);
        }
        assert_eq!(ticks, 5, "the wait's worth of ticks after the first");
        assert!(!w.observe(true, false, tick, 0.45), "it starts over");
    }

    #[test]
    fn the_wielded_weapon_coming_back_resets_the_watch() {
        let mut w = AloneWatch::new();
        for _ in 0..4 {
            assert!(!w.observe(true, false, 0.1, 0.45));
        }
        assert!(!w.observe(true, true, 0.1, 0.45));
        for _ in 0..5 {
            assert!(!w.observe(true, false, 0.1, 0.45));
        }
        assert!(w.observe(true, false, 0.1, 0.45));
        assert!(
            !w.observe(false, false, 0.1, 0.0),
            "no second weapon, nothing to promote"
        );
        assert!(
            w.observe(true, false, f32::NAN, 0.0),
            "at once with no wait"
        );
    }
}
