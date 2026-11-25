//! The recent aimed target across the grip edge, before the snatch is armed.

use crate::controllers::bindings::controls::ButtonControl;

/// A recent offer that a grip press freezes until the consumer takes it.
pub(super) struct Prearm<T> {
    target: Option<T>,
    age_s: f32,
    closed: bool,
}

impl<T> Prearm<T> {
    pub(super) const fn new() -> Self {
        Self {
            target: None,
            age_s: 0.0,
            closed: false,
        }
    }

    pub(super) fn clear(&mut self) {
        *self = Self::new();
    }

    /// A held fist cannot acquire a new remote target after missing its press.
    pub(super) fn observe(
        &mut self,
        current: Option<T>,
        grip: ButtonControl,
        dt: f32,
        valid: impl FnOnce(&mut T) -> bool,
    ) -> Option<&T> {
        self.age_s += if dt.is_finite() {
            dt.max(0.0)
        } else {
            RETAIN_S + 1.0
        };
        let fresh = !self.closed || (grip.pressed && grip.released && !grip.down);
        if !grip.held() {
            self.closed = false;
            if let Some(current) = current {
                self.target = Some(current);
                self.age_s = 0.0;
            }
        } else if fresh {
            self.closed = true;
            if (self.target.is_none() || self.age_s > RETAIN_S)
                && let Some(current) = current
            {
                self.target = Some(current);
                self.age_s = 0.0;
            }
        }
        if self.age_s > RETAIN_S || !self.target.as_mut().is_some_and(valid) {
            self.target = None;
        }
        self.target.as_ref()
    }

    pub(super) fn take(&mut self) -> Option<T> {
        self.target.take()
    }
}

/// Covers a short aim miss at the grip edge without offering an old target indefinitely.
const RETAIN_S: f32 = 0.2;

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: ButtonControl = ButtonControl {
        down: false,
        pressed: false,
        released: false,
    };
    const PRESS: ButtonControl = ButtonControl {
        down: true,
        pressed: true,
        released: false,
    };
    const HELD: ButtonControl = ButtonControl {
        pressed: false,
        ..PRESS
    };

    #[test]
    fn a_missed_aim_at_the_press_keeps_the_recent_identity_for_the_consumer() {
        let mut cache = Prearm::new();
        assert_eq!(cache.observe(Some(1), OPEN, 0.01, |_| true), Some(&1));
        assert_eq!(cache.observe(None, PRESS, 0.03, |_| true), Some(&1));
        assert_eq!(cache.take(), Some(1));
        assert_eq!(cache.observe(Some(2), HELD, 0.01, |_| true), None);
    }

    #[test]
    fn the_grip_edge_keeps_the_previous_aim_instead_of_a_neighbour() {
        let mut cache = Prearm::new();
        cache.observe(Some(1), OPEN, 0.01, |_| true);
        assert_eq!(cache.observe(Some(2), PRESS, 0.01, |_| true), Some(&1));
        assert_eq!(cache.take(), Some(1));
        cache.observe(Some(2), OPEN, 0.01, |_| true);
        assert_eq!(cache.observe(None, PRESS, 0.01, |_| true), Some(&2));
    }

    #[test]
    fn a_press_without_a_recent_target_cannot_acquire_a_remote_target_later() {
        let mut cache = Prearm::new();
        assert_eq!(cache.observe(None, PRESS, 0.01, |_| true), None);
        assert_eq!(cache.observe(Some(2), HELD, 0.01, |_| true), None);
    }

    #[test]
    fn expired_invalid_and_cleared_offers_cannot_arm() {
        for (dt, valid) in [(0.21, true), (0.01, false), (f32::NAN, true)] {
            let mut cache = Prearm::new();
            cache.observe(Some(1), OPEN, 0.01, |_| true);
            assert_eq!(cache.observe(None, PRESS, dt, |_| valid), None);
            assert_eq!(cache.take(), None);
        }
        let mut cache = Prearm::new();
        cache.observe(Some(1), OPEN, 0.01, |_| true);
        cache.clear();
        assert_eq!(cache.observe(None, PRESS, 0.01, |_| true), None);
    }
}
