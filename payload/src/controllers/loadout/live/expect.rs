//! Which hand a weapon the game wields comes into. Pure.
//!
//! The game wields one weapon at a time, and a draw, a pickup, or a take the hands start may wield
//! it only later (a draw through the game's act plays after the current weapon's holster or
//! switch). So a grab leaves its hand expecting the next weapon the game wields ([`Pending`]), or
//! the one it names, and an expectation lapses after a while. A weapon the game wields on its own
//! (the weapon wheel, a switch, an automatic draw) comes into the dominant hand when that is free,
//! otherwise into the other ([`landing`]). The hand is per-hand runtime state beside the table, not
//! an item's place: once the weapon lands, the table holds it in that hand.

use crate::controllers::{
    frame::Hand,
    loadout::{binding::ObjectAddress, transition::TakeSource},
};

/// How long a hand expects a weapon, seconds.
pub(crate) const PENDING_S: f32 = 3.0;

/// What a grab told its hand to expect.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Expect {
    /// A draw off the body.
    Draw,
    /// A take from this source.
    Take(TakeSource),
}

impl Expect {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Expect::Draw => "draw",
            Expect::Take(source) => source.label(),
        }
    }
}

/// A hand expecting the game to wield a weapon.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Pending {
    pub hand: Hand,
    pub expect: Expect,
    /// The weapon the hand is for, when it is known.
    pub weapon: Option<ObjectAddress>,
    age_s: f32,
}

impl Pending {
    pub(crate) const fn new(hand: Hand, expect: Expect, weapon: Option<ObjectAddress>) -> Pending {
        Pending {
            hand,
            expect,
            weapon,
            age_s: 0.0,
        }
    }

    /// The expectation `dt_s` seconds later, or `None` once it has lapsed.
    pub(crate) fn aged(self, dt_s: f32) -> Option<Pending> {
        let dt = if dt_s.is_finite() { dt_s.max(0.0) } else { 0.0 };
        let age_s = self.age_s + dt;
        (age_s <= PENDING_S).then_some(Pending { age_s, ..self })
    }

    /// Whether the expectation is for `object`: the weapon it names, or any when it names none.
    pub(crate) fn claims(self, object: ObjectAddress) -> bool {
        self.weapon.is_none_or(|w| w == object)
    }
}

/// Where the weapon at `object` the game now wields lands, with the hands' expectation `pending`,
/// `free` the hands the table has empty (by [`Hand::index`]), and `dominant` the hand a weapon the
/// game wields on its own goes to first. Returns the hand, the expectation it met (`None` for the
/// game's own draw), and whether that used the expectation up.
pub(crate) fn landing(
    pending: Option<Pending>,
    object: ObjectAddress,
    free: [bool; 2],
    dominant: Hand,
) -> (Hand, Option<Expect>, bool) {
    match pending.filter(|p| p.claims(object)) {
        Some(p) => (p.hand, Some(p.expect), true),
        None => {
            let hand = [dominant, dominant.other()]
                .into_iter()
                .find(|h| free[h.index()])
                .unwrap_or(dominant);
            (hand, None, false)
        }
    }
}

/// The hand a weapon the game gives (`object`) comes into as a rack's copy: the hand left
/// expecting one by a grab of a rack, when it is free (`free`, by [`Hand::index`]) and the
/// expectation is for this weapon or any. `None` for any other give, which the table's give rule
/// places.
pub(crate) fn copy_hand(
    pending: Option<Pending>,
    object: ObjectAddress,
    free: [bool; 2],
) -> Option<Hand> {
    pending
        .filter(|p| {
            matches!(
                p.expect,
                Expect::Take(TakeSource::Rack | TakeSource::RackCopy)
            ) && p.claims(object)
        })
        .map(|p| p.hand)
        .filter(|hand| free[hand.index()])
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::*;

    fn object(n: usize) -> ObjectAddress {
        ObjectAddress::new(NonZeroUsize::MIN.saturating_add(n * 0x10))
    }

    const TICK: f32 = 1.0 / 60.0;

    #[test]
    fn a_draw_lands_in_the_grabbing_hand() {
        for hand in Hand::BOTH {
            let pending = Pending::new(hand, Expect::Draw, None);
            assert_eq!(
                landing(Some(pending), object(1), [true; 2], Hand::Right),
                (hand, Some(Expect::Draw), true)
            );
        }
    }

    #[test]
    fn the_game_s_own_draw_lands_in_the_free_hand_the_dominant_first() {
        assert_eq!(
            landing(None, object(1), [true; 2], Hand::Left),
            (Hand::Left, None, false)
        );
        assert_eq!(
            landing(None, object(1), [false, true], Hand::Left),
            (Hand::Right, None, false),
            "the dominant hand holds the second weapon"
        );
        assert_eq!(
            landing(None, object(1), [false; 2], Hand::Left),
            (Hand::Left, None, false),
            "with both full, the dominant hand, which the table leaves to the game until it empties"
        );
    }

    #[test]
    fn a_take_waits_for_its_own_weapon() {
        let pending = Pending::new(Hand::Left, Expect::Take(TakeSource::World), Some(object(1)));
        assert_eq!(
            landing(Some(pending), object(2), [true; 2], Hand::Right),
            (Hand::Right, None, false),
            "another weapon the game wields first does not take the hand"
        );
        assert_eq!(
            landing(Some(pending), object(1), [true; 2], Hand::Right),
            (Hand::Left, Some(Expect::Take(TakeSource::World)), true)
        );
    }

    #[test]
    fn a_rack_s_copy_comes_into_the_hand_that_asked_for_it() {
        let rack = Pending::new(Hand::Left, Expect::Take(TakeSource::Rack), None);
        assert_eq!(
            copy_hand(Some(rack), object(1), [true; 2]),
            Some(Hand::Left)
        );
        let mut left_full = [true; 2];
        left_full[Hand::Left.index()] = false;
        assert_eq!(
            copy_hand(Some(rack), object(1), left_full),
            None,
            "the hand filled since"
        );
        let draw = Pending::new(Hand::Left, Expect::Draw, None);
        assert_eq!(
            copy_hand(Some(draw), object(1), [true; 2]),
            None,
            "any other give takes the give rule"
        );
        assert_eq!(copy_hand(None, object(1), [true; 2]), None);
    }

    #[test]
    fn an_expectation_lapses() {
        let pending = Pending::new(Hand::Left, Expect::Draw, None);
        assert!(pending.aged(TICK).is_some());
        assert_eq!(pending.aged(PENDING_S + TICK), None);
        assert!(pending.aged(f32::NAN).is_some(), "a bad tick ages nothing");
    }
}
