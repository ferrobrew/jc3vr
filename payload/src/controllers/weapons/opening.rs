//! When a hand holding a grenade or a charge lets it go: when its fingers open, not when its
//! squeeze relaxes. Pure.
//!
//! A throwing hand relaxes its squeeze before the fingers open, and on a force-sensing grip the
//! press ends well before that, so a release read from the grip's press drops the grenade at the
//! feet mid-throw. The fingers are read instead, through the grip's value: on the Index controller
//! the capacitive sense of the fingers round the handle, which stays full while the fingers rest on
//! it however lightly they press, and on a Touch controller the grip's pull. The hand counts as
//! closed once the value passes [`CLOSED`] or the grip is pressed, and opens when the value falls
//! under [`OPEN`]. A hand whose value is not read (an inactive controller) opens with its grip's
//! press instead.

use crate::controllers::{bindings::controls::ButtonControl, frame::Hand};

/// The grip value at or above which the fingers count as closed round the handle.
pub const CLOSED: f32 = 0.6;
/// The grip value under which closed fingers count as opened.
pub const OPEN: f32 = 0.35;

/// Each hand's fingers across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct OpenWatch {
    /// Whether each hand's fingers are closed, by [`Hand::index`].
    closed: [bool; 2],
}

impl OpenWatch {
    pub const fn new() -> Self {
        Self { closed: [false; 2] }
    }

    /// Advance one tick with each hand's grip value (`None` when it is not read) and grip press.
    /// Returns the hands whose fingers opened this tick, by [`Hand::index`].
    pub fn step(&mut self, values: [Option<f32>; 2], grips: [ButtonControl; 2]) -> [bool; 2] {
        Hand::BOTH.map(|hand| {
            let i = hand.index();
            let grip = grips[i];
            let was = self.closed[i];
            let now = match values[i].filter(|v| v.is_finite()) {
                Some(value) if was => value >= OPEN || grip.down,
                Some(value) => value >= CLOSED || grip.held(),
                None => grip.down,
            };
            self.closed[i] = now;
            // A press and release between two ticks, read from the press alone, opens too.
            let tapped = values[i].is_none() && grip.pressed && grip.released && !grip.down;
            (was && !now) || tapped
        })
    }

    #[cfg(test)]
    /// Whether `hand`'s fingers are closed.
    pub fn closed(&self, hand: Hand) -> bool {
        self.closed[hand.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: ButtonControl = ButtonControl {
        down: false,
        pressed: false,
        released: false,
    };
    const HELD: ButtonControl = ButtonControl {
        down: true,
        pressed: false,
        released: false,
    };
    const RELEASE: ButtonControl = ButtonControl {
        down: false,
        pressed: false,
        released: true,
    };

    fn left(value: Option<f32>, grip: ButtonControl) -> ([Option<f32>; 2], [ButtonControl; 2]) {
        ([value, None], [grip, UP])
    }

    #[test]
    fn a_relaxed_squeeze_keeps_the_grenade_until_the_fingers_open() {
        let mut watch = OpenWatch::new();
        let (v, g) = left(Some(1.0), HELD);
        assert_eq!(watch.step(v, g), [false; 2]);
        assert!(watch.closed(Hand::Left));
        // The press ends as the squeeze relaxes, but the fingers still rest on the handle.
        let (v, g) = left(Some(0.9), RELEASE);
        assert_eq!(
            watch.step(v, g),
            [false; 2],
            "a relaxed squeeze is no release"
        );
        let (v, g) = left(Some(0.5), UP);
        assert_eq!(
            watch.step(v, g),
            [false; 2],
            "between the thresholds, still closed"
        );
        let (v, g) = left(Some(0.2), UP);
        assert_eq!(watch.step(v, g), [true, false], "the fingers opened");
        assert!(!watch.closed(Hand::Left));
        // Open stays open: no second release.
        let (v, g) = left(Some(0.1), UP);
        assert_eq!(watch.step(v, g), [false; 2]);
    }

    #[test]
    fn without_a_grip_value_the_press_decides() {
        let mut watch = OpenWatch::new();
        let (v, g) = left(None, HELD);
        watch.step(v, g);
        let (v, g) = left(None, RELEASE);
        assert_eq!(watch.step(v, g), [true, false]);
        // A tap between two ticks opens at once.
        let mut watch = OpenWatch::new();
        let tap = ButtonControl {
            down: false,
            pressed: true,
            released: true,
        };
        let (v, g) = left(None, tap);
        assert_eq!(watch.step(v, g), [true, false]);
    }

    #[test]
    fn a_hand_closes_on_its_press_even_with_a_low_value() {
        let mut watch = OpenWatch::new();
        // A Touch grip pulled just past its press threshold: closed.
        let (v, g) = left(Some(0.45), HELD);
        watch.step(v, g);
        assert!(watch.closed(Hand::Left));
        let (v, g) = left(f32::NAN.into(), UP);
        assert_eq!(
            watch.step(v, g),
            [true, false],
            "an unread value falls back to the press"
        );
    }
}
