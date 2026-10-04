//! Which hand holds which pistol of the dual-wield pair, and the attach bone each pistol rides.
//! Pure.
//!
//! The game draws and holsters the dual-wield slot as one weapon: both pistols come out together
//! and go away together. With independent draws, each hand holds its own pistol: a hand draws its
//! pistol at a hip holster and puts it back there, and a pistol that is not drawn waits on the
//! hip holster on its hand's side while the weapon is wielded ([`bones`]).
//!
//! [`Holding`] follows the game's weapon ([`Holding::observe`]) and the hip grabs. A draw of the
//! slot that a hip grab started ([`Holding::begin_draw`]) brings out only that hand's pistol; a
//! draw the game started on its own (the weapon wheel, a switch, an automatic draw) brings out
//! both, as in the game.

use jc3gi::character::character::SafeBoneIndex;

use crate::controllers::frame::Hand;

/// How long a hip grab's draw waits for the game to wield the pistols, seconds. The game draws
/// the slot after the current weapon's holster or switch has played, so the wait covers a switch.
pub const PENDING_DRAW_S: f32 = 3.0;

/// The pistols each hand holds.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Holding {
    weapon: Option<usize>,
    wielded: bool,
    drawn: [bool; 2],
    pending: Option<PendingDraw>,
}

/// A hip grab's draw the game has not acted on yet.
#[derive(Debug, Copy, Clone, PartialEq)]
struct PendingDraw {
    hand: Hand,
    age_s: f32,
}

impl Holding {
    pub const fn new() -> Self {
        Self {
            weapon: None,
            wielded: false,
            drawn: [false; 2],
            pending: None,
        }
    }

    /// Fold in one input tick: `weapon` is the dual-wield slot's weapon, `wielded` whether the
    /// game has it in the hands, and `dt` the tick's length in seconds. The tick the weapon becomes
    /// wielded draws the pending grab's hand, or both hands for a draw the game started.
    pub fn observe(&mut self, weapon: Option<usize>, wielded: bool, dt: f32) {
        if weapon != self.weapon {
            *self = Self {
                weapon,
                ..Self::new()
            };
        }
        if weapon.is_none() {
            return;
        }
        if wielded && !self.wielded {
            self.drawn = self.next_draw();
            self.pending = None;
        } else if !wielded {
            self.drawn = [false; 2];
            if let Some(p) = &mut self.pending {
                p.age_s += if dt.is_finite() { dt.max(0.0) } else { 0.0 };
                if p.age_s > PENDING_DRAW_S {
                    self.pending = None;
                }
            }
        }
        self.wielded = wielded;
    }

    /// A hip grab by `hand` while the pistols are not wielded: the draw it asks the game for
    /// brings out only `hand`'s pistol.
    pub fn begin_draw(&mut self, hand: Hand) {
        self.pending = Some(PendingDraw { hand, age_s: 0.0 });
    }

    /// Draw `hand`'s pistol while the pistols are wielded. Returns whether it was drawn.
    pub fn draw(&mut self, hand: Hand) -> bool {
        let drawn = &mut self.drawn[hand.index()];
        let changed = self.wielded && !*drawn;
        *drawn |= self.wielded;
        changed
    }

    /// Holster `hand`'s pistol while the other hand still holds its own. Returns whether it was
    /// holstered; the last pistol is holstered by the game's own holster.
    pub fn holster(&mut self, hand: Hand) -> bool {
        let changed = self.wielded && self.drawn[hand.index()] && self.drawn[hand.other().index()];
        if changed {
            self.drawn[hand.index()] = false;
        }
        changed
    }

    /// The hands whose pistols are out: while wielded, the drawn ones; otherwise the hands the
    /// next draw brings out, so that the first placement of a draw already has them.
    pub fn drawn(&self) -> [bool; 2] {
        if self.wielded {
            self.drawn
        } else {
            self.next_draw()
        }
    }

    pub fn wielded(&self) -> bool {
        self.wielded
    }

    /// The hand of the hip grab whose draw is waiting on the game.
    pub fn pending(&self) -> Option<Hand> {
        self.pending.map(|p| p.hand)
    }

    fn next_draw(&self) -> [bool; 2] {
        match self.pending {
            Some(p) => Hand::BOTH.map(|h| h == p.hand),
            None => [true; 2],
        }
    }
}

/// The hand each of the two pistols belongs to, indexed by gun: the first in the dominant hand
/// with `by_handedness`, otherwise the game's layout (the first in the right hand).
pub const fn gun_hands(dominant: Hand, by_handedness: bool) -> [Hand; 2] {
    let first = if by_handedness { dominant } else { Hand::Right };
    [first, first.other()]
}

/// The attach bone (an `NBone::ESafeBoneIndex`) of each pistol, indexed by gun: its hand's attach
/// bone while that hand has it drawn, otherwise the hip holster bone on that hand's side.
pub fn bones(gun_hands: [Hand; 2], drawn: [bool; 2]) -> [u32; 2] {
    gun_hands.map(|hand| {
        let bone = match (hand, drawn[hand.index()]) {
            (Hand::Right, true) => SafeBoneIndex::ATTACH_HAND_RIGHT,
            (Hand::Left, true) => SafeBoneIndex::ATTACH_HAND_LEFT,
            (Hand::Right, false) => SafeBoneIndex::RIGHT_HOLSTER,
            (Hand::Left, false) => SafeBoneIndex::LEFT_HOLSTER,
        };
        bone as u32
    })
}

/// The pistol each hand holds, indexed by [`Hand::index`].
pub fn guns_by_hand(gun_hands: [Hand; 2], drawn: [bool; 2]) -> [Option<usize>; 2] {
    let mut guns = [None; 2];
    for (gun, hand) in gun_hands.into_iter().enumerate() {
        if drawn[hand.index()] {
            guns[hand.index()].get_or_insert(gun);
        }
    }
    guns
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEAPON: Option<usize> = Some(0x1000);
    const TICK: f32 = 1.0 / 60.0;

    fn wielded_by(grab: Option<Hand>) -> Holding {
        let mut h = Holding::new();
        h.observe(WEAPON, false, TICK);
        if let Some(hand) = grab {
            h.begin_draw(hand);
        }
        h.observe(WEAPON, true, TICK);
        h
    }

    #[test]
    fn a_hip_grab_draws_only_its_hand() {
        for hand in Hand::BOTH {
            let h = wielded_by(Some(hand));
            assert_eq!(h.drawn(), Hand::BOTH.map(|x| x == hand));
            assert_eq!(h.pending(), None);
        }
    }

    #[test]
    fn a_draw_the_game_started_brings_out_both() {
        assert_eq!(wielded_by(None).drawn(), [true; 2]);
    }

    #[test]
    fn the_pending_draw_shows_before_the_game_wields() {
        let mut h = Holding::new();
        h.observe(WEAPON, false, TICK);
        h.begin_draw(Hand::Left);
        assert_eq!(h.drawn(), [true, false]);
        assert_eq!(h.pending(), Some(Hand::Left));
    }

    #[test]
    fn a_pending_draw_expires() {
        let mut h = Holding::new();
        h.observe(WEAPON, false, TICK);
        h.begin_draw(Hand::Right);
        h.observe(WEAPON, false, PENDING_DRAW_S + TICK);
        assert_eq!(h.pending(), None);
        h.observe(WEAPON, true, TICK);
        assert_eq!(h.drawn(), [true; 2]);
    }

    #[test]
    fn the_second_hand_draws_and_holsters_its_own() {
        let mut h = wielded_by(Some(Hand::Right));
        assert!(h.draw(Hand::Left));
        assert!(!h.draw(Hand::Left), "a drawn pistol is not drawn again");
        assert_eq!(h.drawn(), [true; 2]);
        assert!(h.holster(Hand::Right));
        assert_eq!(h.drawn(), [true, false]);
        assert!(
            !h.holster(Hand::Left),
            "the last pistol goes through the game's holster"
        );
        assert_eq!(h.drawn(), [true, false]);
    }

    #[test]
    fn nothing_is_drawn_or_holstered_while_not_wielded() {
        let mut h = Holding::new();
        h.observe(WEAPON, false, TICK);
        assert!(!h.draw(Hand::Left));
        assert!(!h.holster(Hand::Left));
        h.observe(WEAPON, true, TICK);
        assert_eq!(h.drawn(), [true; 2]);
    }

    #[test]
    fn holstering_resets_and_the_next_draw_starts_over() {
        let mut h = wielded_by(Some(Hand::Right));
        h.draw(Hand::Left);
        h.observe(WEAPON, false, TICK);
        assert!(!h.wielded());
        h.begin_draw(Hand::Left);
        h.observe(WEAPON, true, TICK);
        assert_eq!(h.drawn(), [true, false]);
    }

    #[test]
    fn a_new_weapon_forgets_the_old_one() {
        let mut h = wielded_by(Some(Hand::Right));
        h.observe(Some(0x2000), true, TICK);
        assert_eq!(h.drawn(), [true; 2]);
        h.observe(None, false, TICK);
        assert!(!h.wielded());
    }

    #[test]
    fn the_first_pistol_follows_the_dominant_hand_when_asked() {
        assert_eq!(gun_hands(Hand::Left, true), [Hand::Left, Hand::Right]);
        assert_eq!(gun_hands(Hand::Left, false), [Hand::Right, Hand::Left]);
        assert_eq!(gun_hands(Hand::Right, true), [Hand::Right, Hand::Left]);
    }

    #[test]
    fn an_undrawn_pistol_waits_on_its_side_s_hip() {
        let hands = gun_hands(Hand::Right, true);
        assert_eq!(
            bones(hands, [true; 2]),
            [
                SafeBoneIndex::ATTACH_HAND_RIGHT as u32,
                SafeBoneIndex::ATTACH_HAND_LEFT as u32
            ]
        );
        assert_eq!(
            bones(hands, [false, true]),
            [
                SafeBoneIndex::ATTACH_HAND_RIGHT as u32,
                SafeBoneIndex::LEFT_HOLSTER as u32
            ]
        );
        assert_eq!(
            bones(gun_hands(Hand::Left, true), [false, true]),
            [
                SafeBoneIndex::LEFT_HOLSTER as u32,
                SafeBoneIndex::ATTACH_HAND_RIGHT as u32
            ]
        );
    }

    #[test]
    fn each_drawn_hand_fires_its_own_pistol() {
        let hands = gun_hands(Hand::Left, true);
        assert_eq!(guns_by_hand(hands, [true; 2]), [Some(0), Some(1)]);
        assert_eq!(guns_by_hand(hands, [false, true]), [None, Some(1)]);
        assert_eq!(guns_by_hand(hands, [false; 2]), [None; 2]);
    }
}
