//! A keyframed scalar over time, eased between its keys.
//!
//! Each segment eases with the quintic smootherstep, so the value, its rate, and its acceleration
//! are continuous across a key: every key is a moment of rest, which is how a body moves between
//! poses (a step starts and stops, a turn starts and stops). A script that wants motion through a
//! point without stopping places its keys further apart.

/// A scalar track: a value at time zero, then keys at increasing times.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    keys: Vec<Key>,
}

impl Track {
    /// A track that starts at `value` at time zero.
    pub fn start(value: f32) -> Track {
        Track {
            keys: vec![Key { t: 0.0, value }],
        }
    }

    /// Ease from the last key's value to `value` over `seconds`.
    ///
    /// # Panics
    ///
    /// When `seconds` is not a positive finite duration: the built-in scripts are code, and such a
    /// key is a mistake in one.
    pub fn ease(mut self, seconds: f32, value: f32) -> Track {
        assert!(
            seconds.is_finite() && seconds > 0.0,
            "track: a key's duration must be positive, not {seconds}"
        );
        let t = self.end() + seconds;
        self.keys.push(Key { t, value });
        self
    }

    /// Stay at the last key's value for `seconds`.
    pub fn hold(self, seconds: f32) -> Track {
        let value = self.last_value();
        self.ease(seconds, value)
    }

    /// Jump to `value` at once: a key an instant after the last one. For inputs, which can change
    /// faster than a body moves.
    pub fn snap(self, value: f32) -> Track {
        self.ease(SNAP_S, value)
    }

    /// The time of the last key, seconds.
    pub fn end(&self) -> f32 {
        self.keys.last().map_or(0.0, |key| key.t)
    }

    /// The value at `t` seconds: the first key's value before it, the last key's after it, and
    /// eased between the two keys around it otherwise.
    pub fn sample(&self, t: f32) -> f32 {
        let after = self.keys.partition_point(|key| key.t <= t);
        if after == 0 {
            return self.keys[0].value;
        }
        if after == self.keys.len() {
            return self.last_value();
        }
        let (a, b) = (self.keys[after - 1], self.keys[after]);
        let s = smootherstep((t - a.t) / (b.t - a.t));
        a.value + (b.value - a.value) * s
    }

    fn last_value(&self) -> f32 {
        self.keys.last().map_or(0.0, |key| key.value)
    }
}

/// The quintic ease of `x` in `0..=1`: zero rate and acceleration at both ends.
pub fn smootherstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Key {
    t: f32,
    value: f32,
}

/// The duration of a [`Track::snap`].
const SNAP_S: f32 = 0.02;

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn walk() -> Track {
        Track::start(0.0).ease(2.0, 1.5).hold(1.0).ease(2.0, 0.0)
    }

    #[test]
    fn the_keys_are_hit_exactly() {
        let track = walk();
        for (t, value) in [(0.0, 0.0), (2.0, 1.5), (2.5, 1.5), (3.0, 1.5), (5.0, 0.0)] {
            assert_eq!(track.sample(t), value, "at {t}");
        }
    }

    #[test]
    fn outside_the_keys_the_value_is_held() {
        let track = walk();
        assert_eq!(track.sample(-1.0), 0.0);
        assert_eq!(track.sample(100.0), 0.0);
        assert_eq!(track.end(), 5.0);
    }

    #[test]
    fn the_midpoint_of_a_segment_is_halfway() {
        assert!((walk().sample(1.0) - 0.75).abs() < 1e-6);
        assert!((walk().sample(4.0) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn a_segment_moves_monotonically() {
        let track = walk();
        let mut last = track.sample(0.0);
        for i in 1..=200 {
            let value = track.sample(i as f32 * 0.01);
            assert!(
                value >= last,
                "at {}: {value} after {last}",
                i as f32 * 0.01
            );
            last = value;
        }
    }

    #[test]
    fn the_rate_is_zero_at_a_key() {
        let track = walk();
        let h = 1e-3;
        for t in [0.0, 2.0, 3.0, 5.0] {
            let rate = (track.sample(t + h) - track.sample(t - h)) / (2.0 * h);
            assert!(rate.abs() < 1e-3, "rate {rate} at {t}");
        }
    }

    #[test]
    fn the_peak_rate_is_the_quintic_ease_s() {
        // The quintic ease peaks at 15/8 of the average rate, halfway through the segment.
        let track = Track::start(0.0).ease(2.0, 1.5);
        let h = 1e-3;
        let rate = (track.sample(1.0 + h) - track.sample(1.0 - h)) / (2.0 * h);
        assert!((rate - 0.75 * 15.0 / 8.0).abs() < 1e-2, "rate {rate}");
    }

    #[test]
    fn a_snap_changes_the_value_almost_at_once() {
        let track = Track::start(0.0).hold(1.0).snap(1.0);
        assert_eq!(track.sample(1.0), 0.0);
        assert_eq!(track.sample(1.0 + SNAP_S), 1.0);
    }

    #[test]
    #[should_panic(expected = "a key's duration must be positive")]
    fn a_key_without_a_duration_is_refused() {
        let _ = Track::start(0.0).ease(0.0, 1.0);
    }
}
