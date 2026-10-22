//! The debug panel's grab: which hand holds the panel, decided once per input tick from each
//! hand's grip and whether its aim ray is on the panel.
//!
//! A hand takes hold when its grip is pressed with its ray on the panel, after a released sample,
//! so a grip already held as the pointer starts never grabs. One hand holds at a time. The grab
//! lasts while the grip is held and the hand stays tracked, and ends when either stops. The
//! placement the grab moves lives in [`crate::hud::egui_panel`].

use crate::{controllers::frame::Hand, hud::pointer::controller::TriggerSample};

/// What one hand offers the grab on one tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct GrabSample {
    pub(super) grip: TriggerSample,
    /// The hand's aim ray is on the panel.
    pub(super) on_panel: bool,
    /// The hand's poses are usable.
    pub(super) usable: bool,
}

/// What the grab does on one tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum GrabStep {
    /// No hand holds the panel.
    Idle,
    /// The hand takes hold this tick.
    Begin(Hand),
    /// The hand keeps holding.
    Hold(Hand),
    /// The hand lets go this tick.
    Release(Hand),
}

impl GrabStep {
    /// The hand that holds the panel after this tick.
    pub(super) fn holding(self) -> Option<Hand> {
        match self {
            GrabStep::Begin(hand) | GrabStep::Hold(hand) => Some(hand),
            GrabStep::Idle | GrabStep::Release(_) => None,
        }
    }
}

/// The grabbing hand and each hand's grip as the grab last saw it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct GrabTracker {
    hand: Option<Hand>,
    /// Each hand's grip on the previous tick, in [`Hand::index`] order, `None` before the first.
    grips: Option<[bool; 2]>,
}

impl GrabTracker {
    pub(super) const fn new() -> Self {
        Self {
            hand: None,
            grips: None,
        }
    }

    /// The hand that holds the panel.
    pub(super) fn hand(&self) -> Option<Hand> {
        self.hand
    }

    /// Advance by one tick. `preferred` takes hold when both hands grab on the same tick, and
    /// `placed` is whether the panel has a placement to take hold of.
    pub(super) fn step(
        &mut self,
        samples: [GrabSample; 2],
        preferred: Hand,
        placed: bool,
    ) -> GrabStep {
        let previous = self.grips;
        let was = |hand: Hand| previous.is_some_and(|grips| grips[hand.index()]);
        let downs = Hand::BOTH.map(|hand| samples[hand.index()].grip.down(was(hand)));
        self.grips = Some(downs);
        if let Some(hand) = self.hand {
            if downs[hand.index()] && samples[hand.index()].usable {
                return GrabStep::Hold(hand);
            }
            self.hand = None;
            return GrabStep::Release(hand);
        }
        if previous.is_none() || !placed {
            return GrabStep::Idle;
        }
        let grabs = |hand: Hand| {
            let sample = samples[hand.index()];
            downs[hand.index()] && !was(hand) && sample.on_panel && sample.usable
        };
        let Some(hand) = [preferred, preferred.other()]
            .into_iter()
            .find(|&h| grabs(h))
        else {
            return GrabStep::Idle;
        };
        self.hand = Some(hand);
        GrabStep::Begin(hand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(grip: bool, on_panel: bool) -> GrabSample {
        GrabSample {
            grip: TriggerSample {
                level: grip,
                pressed: false,
                released: false,
            },
            on_panel,
            usable: true,
        }
    }

    fn step(tracker: &mut GrabTracker, left: GrabSample, right: GrabSample) -> GrabStep {
        tracker.step([left, right], Hand::Right, true)
    }

    #[test]
    fn a_grip_on_the_panel_grabs_until_it_is_released() {
        let mut tracker = GrabTracker::new();
        let rest = sample(false, false);
        assert_eq!(
            step(&mut tracker, rest, sample(false, true)),
            GrabStep::Idle
        );
        assert_eq!(
            step(&mut tracker, rest, sample(true, true)),
            GrabStep::Begin(Hand::Right)
        );
        // The ray may leave the panel while the hand holds it.
        assert_eq!(
            step(&mut tracker, rest, sample(true, false)),
            GrabStep::Hold(Hand::Right)
        );
        assert_eq!(
            step(&mut tracker, rest, sample(false, false)),
            GrabStep::Release(Hand::Right)
        );
        assert_eq!(step(&mut tracker, rest, rest), GrabStep::Idle);
        assert_eq!(tracker.hand(), None);
    }

    #[test]
    fn a_grip_off_the_panel_does_not_grab_when_the_ray_arrives() {
        let mut tracker = GrabTracker::new();
        let rest = sample(false, false);
        step(&mut tracker, rest, rest);
        assert_eq!(
            step(&mut tracker, rest, sample(true, false)),
            GrabStep::Idle
        );
        assert_eq!(step(&mut tracker, rest, sample(true, true)), GrabStep::Idle);
    }

    #[test]
    fn a_grip_held_as_the_tracker_starts_does_not_grab() {
        let mut tracker = GrabTracker::new();
        let rest = sample(false, false);
        assert_eq!(step(&mut tracker, rest, sample(true, true)), GrabStep::Idle);
        assert_eq!(step(&mut tracker, rest, sample(true, true)), GrabStep::Idle);
        step(&mut tracker, rest, sample(false, true));
        assert_eq!(
            step(&mut tracker, rest, sample(true, true)),
            GrabStep::Begin(Hand::Right)
        );
    }

    #[test]
    fn one_hand_holds_at_a_time_and_the_preferred_hand_wins_a_tie() {
        let mut tracker = GrabTracker::new();
        let on = sample(false, true);
        step(&mut tracker, on, on);
        let grip = sample(true, true);
        assert_eq!(step(&mut tracker, grip, grip), GrabStep::Begin(Hand::Right));
        assert_eq!(step(&mut tracker, grip, grip), GrabStep::Hold(Hand::Right));
        // The other hand's grip does not take it over, nor grab when the first lets go.
        assert_eq!(
            step(&mut tracker, grip, sample(false, true)),
            GrabStep::Release(Hand::Right)
        );
        assert_eq!(step(&mut tracker, grip, on), GrabStep::Idle);
    }

    #[test]
    fn losing_tracking_releases_the_grab() {
        let mut tracker = GrabTracker::new();
        let rest = sample(false, false);
        step(&mut tracker, rest, sample(false, true));
        step(&mut tracker, rest, sample(true, true));
        let lost = GrabSample {
            usable: false,
            ..sample(true, true)
        };
        assert_eq!(
            step(&mut tracker, rest, lost),
            GrabStep::Release(Hand::Right)
        );
    }

    #[test]
    fn an_unplaced_panel_cannot_be_grabbed() {
        let mut tracker = GrabTracker::new();
        let rest = sample(false, false);
        tracker.step([rest, sample(false, true)], Hand::Right, false);
        assert_eq!(
            tracker.step([rest, sample(true, true)], Hand::Right, false),
            GrabStep::Idle
        );
    }

    #[test]
    fn a_tap_between_ticks_grabs_and_lets_go() {
        let mut tracker = GrabTracker::new();
        let rest = sample(false, false);
        step(&mut tracker, rest, sample(false, true));
        let tap = GrabSample {
            grip: TriggerSample {
                level: false,
                pressed: true,
                released: true,
            },
            on_panel: true,
            usable: true,
        };
        assert_eq!(step(&mut tracker, rest, tap), GrabStep::Begin(Hand::Right));
        assert_eq!(
            step(&mut tracker, rest, sample(false, true)),
            GrabStep::Release(Hand::Right)
        );
    }
}
