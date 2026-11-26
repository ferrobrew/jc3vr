//! The other hand on the held line: while the line swings its target, the other hand's grip
//! squeezed at the grapple hand's wrist braces it, and its grip on the line ahead of the wrist
//! ([`crate::grapple::line_grab`]) holds it. A braced line is stronger by the swing's brace factor
//! and is anchored between the two hands. A line held ahead is as strong, and is anchored at the
//! hand that holds it, so that hand's pull draws the target in and its swing pulls the target the
//! new way; letting go hands the line back to the grapple hand with what the pull took in.

use glam::Vec3;

use crate::grapple::{line_grab, rope::held_floor};

/// What the other hand is doing this tick, as the brace sees it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct BraceInput {
    /// Whether the other hand's grip is held, closed on nothing.
    pub grip_held: bool,
    /// Whether the other hand is free and empty.
    pub free: bool,
    /// How far the other hand's wrist is from the grapple hand's, metres, or `None` while either
    /// is untracked.
    pub gap_m: Option<f32>,
}

/// How near the wrists must be for a brace to be taken, and how far apart they may drift before it
/// breaks, metres.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct BraceReach {
    pub take_m: f32,
    pub break_m: f32,
}

/// What the other hand's grip on the line ahead of the grapple hand is doing this tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct LineInput {
    /// Whether the other hand's grip holds the line, free and empty, and does not brace it.
    pub holds: bool,
    /// How far ahead of the grapple hand's line origin, along the line, the hand is, metres, or
    /// `None` while it is untracked.
    pub ahead_m: Option<f32>,
    /// The line's held length from its current anchor, metres.
    pub length_m: f32,
}

/// A change of the hand the line is anchored at, with the line's length from the new anchor.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) enum Regrip {
    /// The other hand took hold of the line ahead: the line runs from it.
    Took { length_m: f32 },
    /// The other hand let go: the line runs from the grapple hand again.
    LetGo { length_m: f32 },
}

/// Whether the other hand braces the grapple hand, or holds the line ahead of it.
#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub(super) struct Brace {
    braced: bool,
    /// How far ahead of the grapple hand the other hand took the line, metres, while it holds it.
    taken_ahead_m: Option<f32>,
}

impl Brace {
    pub(super) const fn braced(self) -> bool {
        self.braced
    }

    /// Whether the other hand holds the line ahead of the grapple hand.
    pub(super) const fn holds_line(self) -> bool {
        self.taken_ahead_m.is_some()
    }

    /// Step the other hand's hold of the line ahead of the grapple hand. A hold is taken while the
    /// grip holds the line with the hand tracked, and shortens the line by how far ahead the hand
    /// took it; it lets go when the grip does or the hand loses tracking, giving back how far ahead
    /// the hand let go, at most what it took ([`line_grab::handed_back_m`]). Returns the change of
    /// anchor, if any.
    pub(super) fn step_line(&mut self, input: LineInput) -> Option<Regrip> {
        let ahead = input.ahead_m.filter(|ahead| ahead.is_finite());
        match (self.taken_ahead_m, input.holds) {
            (None, true) => {
                let taken = ahead?.max(0.0);
                self.taken_ahead_m = Some(taken);
                Some(Regrip::Took {
                    length_m: (input.length_m - taken).max(held_floor(0.0)),
                })
            }
            (Some(taken), holds) if !holds || ahead.is_none() => {
                self.taken_ahead_m = None;
                Some(Regrip::LetGo {
                    length_m: line_grab::handed_back_m(input.length_m, taken, ahead, 0.0),
                })
            }
            _ => None,
        }
    }

    /// Step the brace: it is taken while the free other hand's grip is held with its wrist within
    /// the take reach of the grapple hand's, and holds until the grip opens, the hand is no longer
    /// free, a wrist loses tracking, or the wrists drift past the break reach.
    pub(super) fn step(&mut self, input: BraceInput, reach: BraceReach) -> bool {
        let limit = if self.braced {
            reach.break_m.max(reach.take_m)
        } else {
            reach.take_m
        };
        self.braced = input.grip_held
            && input.free
            && input
                .gap_m
                .is_some_and(|gap| gap.is_finite() && gap <= limit);
        self.braced
    }
}

/// Where the line is anchored: the grapple hand's anchor `line_anchor` on its own, or, while the
/// other hand braces at `other_wrist`, the average of the two hands, keeping the grapple hand's
/// offset from its wrist `grapple_wrist` to the line's origin.
pub(super) fn anchor(line_anchor: Vec3, grapple_wrist: Vec3, other_wrist: Option<Vec3>) -> Vec3 {
    match other_wrist.filter(|w| w.is_finite()) {
        Some(other) => line_anchor + (other - grapple_wrist) * 0.5,
        None => line_anchor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REACH: BraceReach = BraceReach {
        take_m: 0.15,
        break_m: 0.3,
    };

    fn at(gap: f32) -> BraceInput {
        BraceInput {
            grip_held: true,
            free: true,
            gap_m: Some(gap),
        }
    }

    #[test]
    fn a_grip_held_at_the_wrist_braces_and_one_held_away_does_not() {
        assert!(Brace::default().step(at(0.1), REACH));
        assert!(!Brace::default().step(at(0.2), REACH));
    }

    #[test]
    fn a_brace_holds_past_the_take_reach_until_the_break_reach() {
        let mut brace = Brace::default();
        assert!(brace.step(at(0.1), REACH));
        assert!(brace.step(at(0.25), REACH));
        assert!(!brace.step(at(0.35), REACH));
        assert!(
            !brace.step(at(0.25), REACH),
            "retaken only within the take reach"
        );
        assert!(brace.step(at(0.12), REACH));
    }

    #[test]
    fn opening_the_grip_taking_something_or_losing_tracking_releases_the_brace() {
        for lost in 0..3 {
            let mut brace = Brace::default();
            assert!(brace.step(at(0.1), REACH));
            let mut input = at(0.1);
            match lost {
                0 => input.grip_held = false,
                1 => input.free = false,
                _ => input.gap_m = None,
            }
            assert!(!brace.step(input, REACH));
            assert!(!brace.braced());
        }
    }

    fn line(holds: bool, ahead_m: Option<f32>, length_m: f32) -> LineInput {
        LineInput {
            holds,
            ahead_m,
            length_m,
        }
    }

    #[test]
    fn a_hand_taking_the_line_ahead_shortens_it_and_its_pull_back_is_kept_on_letting_go() {
        let mut brace = Brace::default();
        assert_eq!(
            brace.step_line(line(true, Some(0.5), 5.0)),
            Some(Regrip::Took { length_m: 4.5 })
        );
        assert!(brace.holds_line());
        assert_eq!(brace.step_line(line(true, Some(0.1), 4.5)), None);
        let Some(Regrip::LetGo { length_m }) = brace.step_line(line(false, Some(-0.2), 4.5)) else {
            panic!("the hand let go");
        };
        assert!((length_m - 4.3).abs() < 1e-6);
        assert!(!brace.holds_line());
        assert_eq!(brace.step_line(line(false, Some(0.5), 4.3)), None);
    }

    #[test]
    fn an_untracked_hand_takes_nothing_and_lets_go_of_what_it_held() {
        let mut brace = Brace::default();
        assert_eq!(brace.step_line(line(true, None, 5.0)), None);
        assert!(!brace.holds_line());
        brace.step_line(line(true, Some(0.4), 5.0));
        assert_eq!(
            brace.step_line(line(true, None, 4.6)),
            Some(Regrip::LetGo { length_m: 5.0 })
        );
    }

    #[test]
    fn a_hand_behind_the_grapple_hand_takes_the_line_at_it() {
        let mut brace = Brace::default();
        assert_eq!(
            brace.step_line(line(true, Some(-0.3), 5.0)),
            Some(Regrip::Took { length_m: 5.0 })
        );
    }

    #[test]
    fn a_braced_line_follows_the_average_of_the_hands() {
        let wrist = Vec3::new(0.0, 1.0, 0.0);
        let line = wrist + Vec3::new(0.0, 0.0, -0.1);
        let other = Vec3::new(0.1, 1.0, 0.0);
        assert_eq!(anchor(line, wrist, None), line);
        let braced = anchor(line, wrist, Some(other));
        assert!((braced - (line + Vec3::X * 0.05)).length() < 1e-6);
        assert_eq!(anchor(line, wrist, Some(Vec3::NAN)), line);
    }
}
