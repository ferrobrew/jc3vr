//! The dual-wielded pistols' magazines, split per hand. Pure.
//!
//! The pair keeps one magazine in the game, so left alone both pistols run dry together and a
//! reload refills both. The split gives each hand a magazine of its own, half the weapon's
//! capacity (the dominant hand takes the odd round), kept as counts beside the game's: the game's
//! magazine is always the sum of the two, so the HUD and every rule of the game's still see one
//! magazine. A shot from a hand takes from that hand's count; an eject returns the hand's rounds to
//! the reserve and takes them from the game's magazine; a load fills the hand's magazine from the
//! reserve and adds it to the game's. When the game's magazine changes in a way the split did not
//! make (the game's own reload, a cheat), the counts are dealt again from the new total.

use crate::controllers::frame::Hand;

/// One hand's magazine.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct HandMagazine {
    pub rounds: i32,
    /// The magazine is out of the pistol.
    pub out: bool,
}

/// The pair's split magazines.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SplitMagazines {
    /// By [`Hand::index`].
    pub hands: [HandMagazine; 2],
    /// The game's magazine as of the split's last change.
    pub total: i32,
}

/// A change the split asks of the game's magazine and reserve.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Transfer {
    /// The game's magazine afterwards.
    pub magazine: i32,
    /// The rounds to add to the reserve (negative to take); zero with unlimited ammunition.
    pub reserve_delta: i32,
}

/// A hand's capacity for a weapon of `capacity` rounds: half, with the odd round in the dominant
/// hand.
pub fn hand_capacity(capacity: i32, hand: Hand, dominant: Hand) -> i32 {
    let capacity = capacity.max(0);
    if hand == dominant {
        capacity - capacity / 2
    } else {
        capacity / 2
    }
}

impl SplitMagazines {
    /// The counts dealt from the game's magazine `total`: the dominant hand's magazine filled first.
    pub fn dealt(total: i32, capacity: i32, dominant: Hand) -> Self {
        let total = total.max(0);
        let mut hands = [HandMagazine::default(); 2];
        let first = total.min(hand_capacity(capacity, dominant, dominant));
        hands[dominant.index()].rounds = first;
        hands[dominant.other().index()].rounds = total - first;
        Self { hands, total }
    }

    /// Bring the split in line with the game's magazine `total`, dealing again when something else
    /// changed it. Returns whether it dealt again.
    pub fn sync(&mut self, total: i32, capacity: i32, dominant: Hand) -> bool {
        if total == self.total {
            return false;
        }
        let outs = self.hands.map(|h| h.out);
        *self = Self::dealt(total, capacity, dominant);
        // A magazine the game filled is in, whatever the split thought.
        if total == 0 {
            for (hand, out) in self.hands.iter_mut().zip(outs) {
                hand.out = out;
            }
        }
        true
    }

    /// A shot from `hand`, after which the game's magazine is `total`.
    pub fn on_shot(&mut self, hand: Hand, total: i32, capacity: i32, dominant: Hand) {
        let rounds = &mut self.hands[hand.index()].rounds;
        if *rounds > 0 && total == self.total - 1 {
            *rounds -= 1;
            self.total = total;
        } else {
            self.sync(total, capacity, dominant);
        }
    }

    /// Whether `hand`'s pistol can fire.
    pub fn can_fire(&self, hand: Hand) -> bool {
        let h = self.hands[hand.index()];
        !h.out && h.rounds > 0
    }

    /// Eject `hand`'s magazine; `None` when it is out already.
    pub fn eject(&mut self, hand: Hand, unlimited: bool) -> Option<Transfer> {
        let h = &mut self.hands[hand.index()];
        if h.out {
            return None;
        }
        let rounds = h.rounds;
        h.rounds = 0;
        h.out = true;
        self.total -= rounds;
        Some(Transfer {
            magazine: self.total,
            reserve_delta: if unlimited { 0 } else { rounds },
        })
    }

    /// Load a fresh magazine into `hand`'s pistol from a reserve of `reserve` rounds, after
    /// dropping any magazine still in it. `None` when there is nothing to load.
    pub fn load(
        &mut self,
        hand: Hand,
        capacity: i32,
        dominant: Hand,
        reserve: i32,
        unlimited: bool,
    ) -> Option<Transfer> {
        let returned = self.eject(hand, unlimited).map_or(0, |t| t.reserve_delta);
        let available = if unlimited {
            i32::MAX
        } else {
            reserve.max(0) + returned
        };
        let rounds = hand_capacity(capacity, hand, dominant).min(available);
        if rounds <= 0 {
            return (returned != 0).then_some(Transfer {
                magazine: self.total,
                reserve_delta: returned,
            });
        }
        let h = &mut self.hands[hand.index()];
        h.rounds = rounds;
        h.out = false;
        self.total += rounds;
        Some(Transfer {
            magazine: self.total,
            reserve_delta: if unlimited { 0 } else { returned - rounds },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOM: Hand = Hand::Right;

    #[test]
    fn the_capacity_splits_with_the_odd_round_in_the_dominant_hand() {
        assert_eq!(hand_capacity(15, Hand::Right, DOM), 8);
        assert_eq!(hand_capacity(15, Hand::Left, DOM), 7);
        assert_eq!(hand_capacity(16, Hand::Left, DOM), 8);
        let s = SplitMagazines::dealt(10, 15, DOM);
        assert_eq!(s.hands[Hand::Right.index()].rounds, 8);
        assert_eq!(s.hands[Hand::Left.index()].rounds, 2);
    }

    #[test]
    fn a_hand_runs_dry_on_its_own() {
        let mut s = SplitMagazines::dealt(3, 4, DOM);
        assert_eq!(s.hands[Hand::Left.index()].rounds, 1);
        s.on_shot(Hand::Left, 2, 4, DOM);
        assert!(!s.can_fire(Hand::Left));
        assert!(s.can_fire(Hand::Right));
        assert_eq!(s.total, 2);
    }

    #[test]
    fn an_eject_returns_the_hands_rounds_and_a_load_fills_only_that_hand() {
        let mut s = SplitMagazines::dealt(12, 15, DOM);
        let t = s.eject(Hand::Left, false).unwrap();
        assert_eq!(
            t,
            Transfer {
                magazine: 8,
                reserve_delta: 4
            }
        );
        assert!(s.eject(Hand::Left, false).is_none());
        assert!(!s.can_fire(Hand::Left));
        let t = s.load(Hand::Left, 15, DOM, 30, false).unwrap();
        assert_eq!(
            t,
            Transfer {
                magazine: 15,
                reserve_delta: -7
            }
        );
        assert!(s.can_fire(Hand::Left));
    }

    #[test]
    fn a_load_with_the_magazine_still_in_swaps_it() {
        let mut s = SplitMagazines::dealt(10, 15, DOM);
        let t = s.load(Hand::Left, 15, DOM, 0, false).unwrap();
        // The two rounds came back to the reserve and went straight into the fresh magazine.
        assert_eq!(
            t,
            Transfer {
                magazine: 10,
                reserve_delta: 0
            }
        );
        let t = s.load(Hand::Right, 15, DOM, 5, true).unwrap();
        assert_eq!(t.reserve_delta, 0);
        assert_eq!(s.hands[Hand::Right.index()].rounds, 8);
    }

    #[test]
    fn a_short_reserve_loads_what_it_has_and_an_empty_one_nothing() {
        let mut s = SplitMagazines::dealt(0, 15, DOM);
        s.hands[Hand::Left.index()].out = true;
        let t = s.load(Hand::Left, 15, DOM, 3, false).unwrap();
        assert_eq!(
            t,
            Transfer {
                magazine: 3,
                reserve_delta: -3
            }
        );
        let mut s = SplitMagazines::dealt(0, 15, DOM);
        s.hands[Hand::Left.index()].out = true;
        assert!(s.load(Hand::Left, 15, DOM, 0, false).is_none());
    }

    #[test]
    fn a_change_the_split_did_not_make_deals_again() {
        let mut s = SplitMagazines::dealt(2, 15, DOM);
        s.eject(Hand::Left, false);
        assert!(s.sync(15, 15, DOM));
        assert!(s.can_fire(Hand::Left));
        assert!(s.can_fire(Hand::Right));
        assert!(!s.sync(15, 15, DOM));
    }
}
