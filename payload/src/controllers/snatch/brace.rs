//! The two-handed brace on the held line: while the line swings its target, the other hand's grip
//! squeezed at the grapple hand's wrist braces it. A braced line is stronger by the swing's brace
//! factor and is anchored between the two hands.

use glam::Vec3;

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

/// Whether the other hand braces the grapple hand.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub(super) struct Brace {
    braced: bool,
}

impl Brace {
    pub(super) const fn braced(self) -> bool {
        self.braced
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
