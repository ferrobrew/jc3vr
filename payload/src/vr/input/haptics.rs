//! The haptics request queue. Any thread queues a pulse with [`request`]; the VR frame loop flushes
//! the queue once per rendered frame under the runtime lock, where the session is available.
//!
//! OpenXR plays one vibration per controller at a time, and a new one replaces whatever is
//! playing. The queue therefore holds at most one pending pulse per hand, and requests arriving
//! between two flushes merge into it ([`HapticPulse::merge`]), so a burst of requests (every
//! pellet of a shotgun blast) plays as one pulse at the strongest amplitude rather than as the
//! last request alone.

use std::time::Duration;

use parking_lot::Mutex;

use crate::controllers::frame::Hand;

/// A vibration pulse.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HapticPulse {
    pub duration: Duration,
    /// The vibration frequency, hertz, or `None` for the runtime's default.
    pub frequency_hz: Option<f32>,
    /// `0..=1`.
    pub amplitude: f32,
}

impl HapticPulse {
    /// Combine two pulses requested in the same frame: the longer duration, the stronger
    /// amplitude, and the frequency of the stronger pulse.
    pub fn merge(self, other: HapticPulse) -> HapticPulse {
        let stronger = if other.amplitude > self.amplitude {
            other
        } else {
            self
        };
        HapticPulse {
            duration: self.duration.max(other.duration),
            frequency_hz: stronger.frequency_hz,
            amplitude: stronger.amplitude,
        }
    }
}

/// Queue a pulse on `hand`, to play at the next rendered frame. Dropped if the controllers or
/// haptics are disabled at that frame.
pub fn request(hand: Hand, pulse: HapticPulse) {
    let mut pending = PENDING.lock();
    let slot = &mut pending[hand.index()];
    *slot = Some(match *slot {
        Some(queued) => queued.merge(pulse),
        None => pulse,
    });
}

/// Take the pending pulses, indexed by [`Hand::index`].
pub(crate) fn take() -> [Option<HapticPulse>; 2] {
    std::mem::take(&mut *PENDING.lock())
}

static PENDING: Mutex<[Option<HapticPulse>; 2]> = Mutex::new([None, None]);

#[cfg(test)]
mod tests {
    use super::*;

    fn pulse(ms: u64, frequency_hz: Option<f32>, amplitude: f32) -> HapticPulse {
        HapticPulse {
            duration: Duration::from_millis(ms),
            frequency_hz,
            amplitude,
        }
    }

    #[test]
    fn merging_keeps_the_longest_and_the_strongest() {
        let merged = pulse(20, Some(160.0), 0.3).merge(pulse(80, Some(60.0), 0.9));
        assert_eq!(merged, pulse(80, Some(60.0), 0.9));
        let merged = pulse(80, None, 0.9).merge(pulse(20, Some(60.0), 0.3));
        assert_eq!(merged, pulse(80, None, 0.9));
    }
}
