//! Which hand holds the wielded weapon. Pure.
//!
//! [`Holder`] follows the local player's wielded weapon ([`Holder::observe`]) and the grabs that
//! decide its hand:
//!
//! - a holster grab's draw ([`Cause::Draw`]) and a pickup ([`Cause::Pickup`]) leave a pending
//!   hand, which the next weapon to be wielded takes. A pending hand waits a while for the game
//!   (a draw plays after the current weapon's holster or switch) and then lapses;
//! - a weapon the game wields on its own (the weapon wheel, a switch, an automatic draw) goes to
//!   the fallback hand the caller names, the acting hand;
//! - a handover ([`Holder::hand_over`]) moves the wielded weapon to the other hand at once.
//!
//! A weapon that leaves the hands forgets its hand. A pickup names the weapon it is for, so a
//! pending pickup is not taken by another weapon the game wields first.

use crate::controllers::frame::Hand;

/// How long a pending hand waits for the game to wield a weapon, seconds.
pub const PENDING_S: f32 = 3.0;

/// What left a pending hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Cause {
    /// A holster grab's draw.
    Draw,
    /// A weapon taken off the ground.
    Pickup,
    /// A gun taken out of a living character's hands.
    Steal,
}

impl Cause {
    pub const fn label(self) -> &'static str {
        match self {
            Cause::Draw => "draw",
            Cause::Pickup => "pickup",
            Cause::Steal => "taken from a character",
        }
    }
}

/// A hand waiting for the game to wield a weapon.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Pending {
    pub hand: Hand,
    pub cause: Cause,
    /// The weapon the hand is for, when it is known.
    pub weapon: Option<usize>,
    age_s: f32,
}

/// The hand holding the wielded weapon.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Holder {
    /// The wielded weapon's address, while one is wielded.
    weapon: Option<usize>,
    hand: Option<Hand>,
    /// What brought the wielded weapon into the hands, or `None` for the game's own draw.
    origin: Option<Cause>,
    pending: Option<Pending>,
}

/// What a tick of [`Holder::observe`] changed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Change {
    /// A weapon came into `hand`, from a pending hand of `cause`, or from the fallback.
    Took { hand: Hand, cause: Option<Cause> },
    /// The weapon left the hands.
    Left,
}

impl Holder {
    pub const fn new() -> Self {
        Self {
            weapon: None,
            hand: None,
            origin: None,
            pending: None,
        }
    }

    /// Fold in one input tick: `wielded` is the address of the weapon in the hands, `fallback` the
    /// hand a weapon the game wields on its own goes to, and `dt` the tick's length in seconds.
    pub fn observe(&mut self, wielded: Option<usize>, fallback: Hand, dt: f32) -> Option<Change> {
        if let Some(p) = &mut self.pending {
            p.age_s += if dt.is_finite() { dt.max(0.0) } else { 0.0 };
            if p.age_s > PENDING_S {
                self.pending = None;
            }
        }
        if wielded == self.weapon {
            return None;
        }
        self.weapon = wielded;
        let Some(address) = wielded else {
            self.hand = None;
            self.origin = None;
            return Some(Change::Left);
        };
        let pending = self
            .pending
            .filter(|p| p.weapon.is_none_or(|w| w == address));
        if pending.is_some() {
            self.pending = None;
        }
        let hand = pending.map_or(fallback, |p| p.hand);
        self.hand = Some(hand);
        self.origin = pending.map(|p| p.cause);
        Some(Change::Took {
            hand,
            cause: pending.map(|p| p.cause),
        })
    }

    /// Leave `hand` pending for the next weapon wielded, or for `weapon` when it is named.
    pub fn begin(&mut self, hand: Hand, cause: Cause, weapon: Option<usize>) {
        self.pending = Some(Pending {
            hand,
            cause,
            weapon,
            age_s: 0.0,
        });
    }

    /// Hand the wielded weapon to `hand`. Returns whether it moved: a weapon is wielded and the
    /// other hand held it.
    pub fn hand_over(&mut self, hand: Hand) -> bool {
        let moved = self.weapon.is_some() && self.hand == Some(hand.other());
        if moved {
            self.hand = Some(hand);
        }
        moved
    }

    /// The hand holding the wielded weapon.
    pub fn hand(&self) -> Option<Hand> {
        self.hand
    }

    pub fn pending(&self) -> Option<Pending> {
        self.pending
    }

    /// What brought the wielded weapon into the hands, or `None` for the game's own draw.
    pub fn origin(&self) -> Option<Cause> {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIFLE: usize = 0x1000;
    const PISTOL: usize = 0x2000;
    const TICK: f32 = 1.0 / 60.0;

    #[test]
    fn a_draw_goes_to_the_grabbing_hand() {
        for hand in Hand::BOTH {
            let mut h = Holder::new();
            h.observe(None, Hand::Right, TICK);
            h.begin(hand, Cause::Draw, None);
            assert_eq!(
                h.observe(Some(RIFLE), Hand::Right, TICK),
                Some(Change::Took {
                    hand,
                    cause: Some(Cause::Draw)
                })
            );
            assert_eq!(h.hand(), Some(hand));
            assert_eq!(h.pending(), None);
        }
    }

    #[test]
    fn a_draw_the_game_started_goes_to_the_fallback() {
        let mut h = Holder::new();
        assert_eq!(
            h.observe(Some(RIFLE), Hand::Left, TICK),
            Some(Change::Took {
                hand: Hand::Left,
                cause: None
            })
        );
    }

    #[test]
    fn a_pending_hand_lapses() {
        let mut h = Holder::new();
        h.begin(Hand::Left, Cause::Draw, None);
        h.observe(None, Hand::Right, PENDING_S + TICK);
        assert_eq!(h.pending(), None);
        h.observe(Some(RIFLE), Hand::Right, TICK);
        assert_eq!(h.hand(), Some(Hand::Right));
    }

    #[test]
    fn a_pickup_waits_for_its_own_weapon() {
        let mut h = Holder::new();
        h.begin(Hand::Left, Cause::Pickup, Some(RIFLE));
        // Another weapon the game wields first does not take the pickup's hand.
        h.observe(Some(PISTOL), Hand::Right, TICK);
        assert_eq!(h.hand(), Some(Hand::Right));
        assert!(h.pending().is_some());
        h.observe(Some(RIFLE), Hand::Right, TICK);
        assert_eq!(h.hand(), Some(Hand::Left));
    }

    #[test]
    fn a_pickup_swaps_the_weapon_in_the_hands() {
        let mut h = Holder::new();
        h.observe(Some(PISTOL), Hand::Right, TICK);
        h.begin(Hand::Left, Cause::Pickup, Some(RIFLE));
        assert_eq!(
            h.observe(Some(RIFLE), Hand::Right, TICK),
            Some(Change::Took {
                hand: Hand::Left,
                cause: Some(Cause::Pickup)
            })
        );
    }

    #[test]
    fn a_handover_moves_the_weapon_to_the_free_hand() {
        let mut h = Holder::new();
        h.observe(Some(RIFLE), Hand::Right, TICK);
        assert!(!h.hand_over(Hand::Right), "the holding hand already has it");
        assert!(h.hand_over(Hand::Left));
        assert_eq!(h.hand(), Some(Hand::Left));
        assert_eq!(h.observe(Some(RIFLE), Hand::Right, TICK), None);
        assert_eq!(
            h.hand(),
            Some(Hand::Left),
            "the weapon stays where it was put"
        );
    }

    #[test]
    fn nothing_is_handed_over_with_nothing_wielded() {
        let mut h = Holder::new();
        assert!(!h.hand_over(Hand::Left));
        assert_eq!(h.hand(), None);
    }

    #[test]
    fn the_origin_follows_what_brought_the_weapon() {
        let mut h = Holder::new();
        h.begin(Hand::Left, Cause::Steal, Some(RIFLE));
        h.observe(Some(RIFLE), Hand::Right, TICK);
        assert_eq!(h.origin(), Some(Cause::Steal));
        assert!(h.hand_over(Hand::Right));
        assert_eq!(
            h.origin(),
            Some(Cause::Steal),
            "a handover keeps the origin"
        );
        h.begin(Hand::Right, Cause::Pickup, None);
        h.observe(Some(PISTOL), Hand::Right, TICK);
        assert_eq!(h.origin(), Some(Cause::Pickup));
        h.observe(None, Hand::Right, TICK);
        assert_eq!(
            h.origin(),
            None,
            "a weapon that left the hands forgets its origin"
        );
        h.observe(Some(RIFLE), Hand::Right, TICK);
        assert_eq!(h.origin(), None, "the game's own draw has no origin");
    }

    #[test]
    fn a_holstered_weapon_forgets_its_hand() {
        let mut h = Holder::new();
        h.begin(Hand::Left, Cause::Draw, None);
        h.observe(Some(RIFLE), Hand::Right, TICK);
        assert_eq!(h.observe(None, Hand::Right, TICK), Some(Change::Left));
        assert_eq!(h.hand(), None);
        h.observe(Some(RIFLE), Hand::Right, TICK);
        assert_eq!(
            h.hand(),
            Some(Hand::Right),
            "the next draw the game starts goes to the fallback"
        );
    }
}
