//! The eject triggers: a tap of the reload button, and in a vehicle seat a double tap of the fire
//! press. Pure.
//!
//! The reload button doubles as the holster (the game holsters on a held press), so a press ejects
//! only when it is released within the tap time; a hold that runs past it ejects nothing.
//!
//! A tap ejects the magazine of the driest gun in either hand: the one with the fewest rounds in its
//! magazine, the dominant hand's on a tie ([`tap_target`]). A gun whose magazine is already out, or
//! already on its way out, is not a candidate, so a second tap ejects the other gun's.

use jc3vr_body::controllers::intent::controls::ButtonControl;
use jc3vr_input::frame::Hand;

/// The reload button's tap detector.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct TapDetector {
    /// How long the button has been down, while it is.
    held_s: Option<f32>,
}

impl TapDetector {
    pub const fn new() -> Self {
        Self { held_s: None }
    }

    /// Fold in one tick of the button; true when a tap ends this tick.
    /// A press and release inside one tick is a tap of no length.
    pub fn step(&mut self, button: ButtonControl, dt: f32, tap_s: f32) -> bool {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let mut tapped = false;
        if button.pressed && self.held_s.is_none() {
            self.held_s = Some(0.0);
        } else if let Some(held) = &mut self.held_s {
            *held += dt;
        }
        if (button.released || !button.held())
            && let Some(held) = self.held_s.take()
        {
            tapped = held <= tap_s;
        }
        tapped
    }
}

/// A double tap of a gun hand's fire press: two taps, each no longer than the tap, the second
/// ending within the gap of the first. The seated magazine release, where the hands
/// are on the vehicle's controls and the reload button is not bound.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct DoubleTapDetector {
    tap: TapDetector,
    /// The seconds since the first tap ended, while a second may still follow.
    since_first_s: Option<f32>,
}

impl DoubleTapDetector {
    pub const fn new() -> Self {
        Self {
            tap: TapDetector::new(),
            since_first_s: None,
        }
    }

    /// Fold in one tick of the press; true when the second tap of a double tap ends this tick.
    /// `gap_s` is the longest from the end of the first tap to the end of the second.
    pub fn step(&mut self, button: ButtonControl, dt: f32, tap_s: f32, gap_s: f32) -> bool {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if let Some(since) = &mut self.since_first_s {
            *since += dt;
            if *since > gap_s {
                self.since_first_s = None;
            }
        }
        if !self.tap.step(button, dt, tap_s) {
            return false;
        }
        if self.since_first_s.take().is_some() {
            return true;
        }
        self.since_first_s = Some(0.0);
        false
    }
}

/// The hand whose gun a tap ejects, from the rounds in each hand's ejectable magazine by
/// [`Hand::index`] (`None` for a hand with no gun, or whose gun has no magazine to eject) and the
/// `dominant` hand: the fewest rounds, the dominant hand's on a tie.
pub(crate) fn tap_target(rounds: [Option<i32>; 2], dominant: Hand) -> Option<Hand> {
    [dominant, dominant.other()]
        .into_iter()
        .filter_map(|hand| Some((hand, rounds[hand.index()]?)))
        .min_by_key(|&(_, rounds)| rounds)
        .map(|(hand, _)| hand)
}

#[cfg(test)]
mod tests {

    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn button(down: bool, pressed: bool, released: bool) -> ButtonControl {
        ButtonControl {
            down,
            pressed,
            released,
        }
    }

    #[test]
    fn a_short_press_taps_on_release() {
        let mut t = TapDetector::new();
        assert!(!t.step(button(true, true, false), DT, 0.3));
        assert!(!t.step(button(true, false, false), DT, 0.3));
        assert!(t.step(button(false, false, true), DT, 0.3));
        assert!(!t.step(button(false, false, false), DT, 0.3));
    }

    #[test]
    fn a_hold_for_the_holster_never_taps() {
        let mut t = TapDetector::new();
        t.step(button(true, true, false), DT, 0.3);
        for _ in 0..30 {
            assert!(!t.step(button(true, false, false), DT, 0.3));
        }
        assert!(!t.step(button(false, false, true), DT, 0.3));
    }

    #[test]
    fn a_press_and_release_within_one_tick_taps() {
        let mut t = TapDetector::new();
        assert!(t.step(button(false, true, true), DT, 0.3));
        assert!(!t.step(button(false, false, false), DT, 0.3));
    }

    #[test]
    fn a_tap_ejects_the_driest_gun() {
        assert_eq!(
            tap_target([Some(3), Some(12)], Hand::Right),
            Some(Hand::Left)
        );
        assert_eq!(
            tap_target([Some(12), Some(0)], Hand::Left),
            Some(Hand::Right)
        );
    }

    #[test]
    fn a_tie_ejects_the_dominant_hands_gun() {
        assert_eq!(tap_target([Some(5); 2], Hand::Right), Some(Hand::Right));
        assert_eq!(tap_target([Some(5); 2], Hand::Left), Some(Hand::Left));
    }

    #[test]
    fn a_hand_without_a_magazine_to_eject_is_passed_over() {
        // The second tap of a double tap: the first gun's magazine is already out.
        assert_eq!(tap_target([None, Some(30)], Hand::Right), Some(Hand::Right));
        assert_eq!(tap_target([Some(30), None], Hand::Right), Some(Hand::Left));
        assert_eq!(tap_target([None; 2], Hand::Right), None);
    }

    #[test]
    fn two_quick_taps_are_a_double_tap_and_a_slow_pair_is_not() {
        let press = ButtonControl {
            down: true,
            pressed: true,
            released: false,
        };
        let release = ButtonControl {
            down: false,
            pressed: false,
            released: true,
        };
        let up = ButtonControl::default();
        let mut d = DoubleTapDetector::new();
        assert!(!d.step(press, 0.03, 0.3, 0.4));
        assert!(!d.step(release, 0.03, 0.3, 0.4));
        assert!(!d.step(up, 0.1, 0.3, 0.4));
        assert!(!d.step(press, 0.03, 0.3, 0.4));
        assert!(d.step(release, 0.03, 0.3, 0.4));

        let mut d = DoubleTapDetector::new();
        d.step(press, 0.03, 0.3, 0.4);
        d.step(release, 0.03, 0.3, 0.4);
        d.step(up, 0.5, 0.3, 0.4);
        d.step(press, 0.03, 0.3, 0.4);
        assert!(!d.step(release, 0.03, 0.3, 0.4), "too far apart");
    }
}
