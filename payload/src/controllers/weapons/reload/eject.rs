//! The eject triggers: a tap of the reload button and a flick of the gun hand. Pure.
//!
//! The reload button doubles as the holster (the game holsters on a held press), so a press ejects
//! only when it is released within the tap time; a hold that runs past it ejects nothing. A flick
//! is the gun hand turning faster than the flick speed, measured from the change in the hand's
//! orientation between two ticks, and it ejects once per cooldown.
//!
//! A tap ejects the dominant hand's gun when it holds one, and otherwise the other hand's
//! ([`tap_targets`]). A flick ejects the gun of the hand that flicked.

use glam::Quat;

use crate::controllers::{bindings::controls::ButtonControl, frame::Hand};

/// The reload button's tap detector.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TapDetector {
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

/// One gun hand's flick detector.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct FlickDetector {
    last: Option<Quat>,
    /// Seconds since the last flick, while within the cooldown.
    since_flick_s: Option<f32>,
    /// The angular speed over the last tick, degrees per second.
    pub speed_dps: f32,
}

impl FlickDetector {
    pub const fn new() -> Self {
        Self {
            last: None,
            since_flick_s: None,
            speed_dps: 0.0,
        }
    }

    /// Fold in the hand's orientation this tick (`None` without a tracked hand); true on a flick.
    pub fn step(
        &mut self,
        orientation: Option<Quat>,
        dt: f32,
        speed_dps: f32,
        cooldown_s: f32,
    ) -> bool {
        if let Some(since) = &mut self.since_flick_s {
            *since += dt.max(0.0);
            if *since >= cooldown_s {
                self.since_flick_s = None;
            }
        }
        let Some(now) = orientation.filter(|q| q.is_finite()) else {
            self.last = None;
            self.speed_dps = 0.0;
            return false;
        };
        let speed = match self.last {
            Some(last) if dt > 1e-4 && dt.is_finite() => {
                (now * last.inverse())
                    .normalize()
                    .angle_between(Quat::IDENTITY)
                    / dt
            }
            _ => 0.0,
        };
        self.last = Some(now);
        self.speed_dps = speed.to_degrees();
        if self.since_flick_s.is_none() && self.speed_dps >= speed_dps {
            self.since_flick_s = Some(0.0);
            return true;
        }
        false
    }

    /// Forget the last orientation, so that a jump in tracking does not read as a flick.
    pub fn reset(&mut self) {
        self.last = None;
        self.speed_dps = 0.0;
    }
}

/// The hands whose guns a tap ejects, by [`Hand::index`], from the hands that hold a gun (`guns`)
/// and the `dominant` hand: the dominant hand's gun when it holds one, otherwise the other hand's.
pub fn tap_targets(guns: [bool; 2], dominant: Hand) -> [bool; 2] {
    let tapped = [dominant, dominant.other()]
        .into_iter()
        .find(|hand| guns[hand.index()]);
    Hand::BOTH.map(|hand| Some(hand) == tapped)
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

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

    fn turn(deg: f32) -> Quat {
        Quat::from_axis_angle(Vec3::X, deg.to_radians())
    }

    #[test]
    fn a_fast_turn_flicks_once_per_cooldown() {
        let mut f = FlickDetector::new();
        assert!(!f.step(Some(turn(0.0)), DT, 900.0, 0.5));
        // 20 degrees in a sixtieth of a second is 1200 degrees per second.
        assert!(f.step(Some(turn(20.0)), DT, 900.0, 0.5));
        assert!(f.speed_dps > 1100.0);
        assert!(!f.step(Some(turn(40.0)), DT, 900.0, 0.5));
        for i in 0..40 {
            f.step(Some(turn(40.0 + i as f32 * 0.1)), DT, 900.0, 0.5);
        }
        assert!(f.step(Some(turn(70.0)), DT, 900.0, 0.5));
    }

    #[test]
    fn ordinary_aiming_and_lost_tracking_never_flick() {
        let mut f = FlickDetector::new();
        for i in 0..60 {
            assert!(!f.step(Some(turn(i as f32 * 3.0)), DT, 900.0, 0.5));
        }
        assert!(!f.step(None, DT, 900.0, 0.5));
        // The first sample after tracking returns has nothing to compare with.
        assert!(!f.step(Some(turn(170.0)), DT, 900.0, 0.5));
    }

    #[test]
    fn a_tap_ejects_the_dominant_hands_gun_first() {
        assert_eq!(tap_targets([true; 2], Hand::Right), [false, true]);
        assert_eq!(tap_targets([true; 2], Hand::Left), [true, false]);
    }

    #[test]
    fn a_tap_ejects_the_other_hands_gun_when_the_dominant_hand_has_none() {
        assert_eq!(tap_targets([true, false], Hand::Right), [true, false]);
        assert_eq!(tap_targets([false; 2], Hand::Right), [false, false]);
    }
}
