//! The game's own on-foot speeds, measured and logged once each: for every target speed the input
//! tasks ask for (walk, non-combat walk, run, backpedal), the animation's root speed while the
//! stick holds it. The stick locomotion takes the target speed as its own, so these lines say how
//! closely the clips' real speed matched it.

use std::time::{Duration, Instant};

/// Watches the target speed and the root speed, and logs each target speed once.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedLog {
    /// The target speed being held, in centimetres per second, since when, and the root speed's
    /// running sum and sample count.
    current: Option<Held>,
    /// The target speeds already logged, in centimetres per second.
    logged: [Option<i32>; MAX_LOGGED],
}

impl SpeedLog {
    pub const fn new() -> Self {
        Self {
            current: None,
            logged: [None; MAX_LOGGED],
        }
    }

    /// Fold in one tick, and log a target speed held long enough that has not been logged yet.
    pub fn observe(&mut self, now: Instant, stick: f32, target: Option<f32>, root_speed: f32) {
        if let Some(measured) = self.fold(now, stick, target, root_speed) {
            tracing::info!(
                target: "controllers",
                target_mps = measured.target_mps,
                root_mps = measured.root_mps,
                held_s = measured.held.as_secs_f32(),
                "locomotion: the game's on-foot speed",
            );
        }
    }

    fn fold(
        &mut self,
        now: Instant,
        stick: f32,
        target: Option<f32>,
        root_speed: f32,
    ) -> Option<Measured> {
        let key = target
            .filter(|t| t.is_finite() && *t > 0.0 && stick >= MIN_STICK && root_speed.is_finite())
            .map(|t| (t * 100.0).round() as i32);
        let Some(key) = key else {
            self.current = None;
            return None;
        };
        let held = match &mut self.current {
            Some(held) if held.key == key => held,
            _ => self.current.insert(Held {
                key,
                since: now,
                sum: 0.0,
                count: 0,
            }),
        };
        // The first second of a hold is the start clip's ramp, not the speed.
        let held_for = now.saturating_duration_since(held.since);
        if held_for < SETTLE {
            return None;
        }
        held.sum += root_speed;
        held.count += 1;
        if held_for < SETTLE + MEASURE || self.logged.contains(&Some(key)) {
            return None;
        }
        let measured = Measured {
            target_mps: key as f32 / 100.0,
            root_mps: held.sum / held.count as f32,
            held: held_for,
        };
        if let Some(slot) = self.logged.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(key);
        }
        Some(measured)
    }
}

impl Default for SpeedLog {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Held {
    key: i32,
    since: Instant,
    sum: f32,
    count: u32,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Measured {
    target_mps: f32,
    root_mps: f32,
    held: Duration,
}

/// How many distinct target speeds are logged in a session.
const MAX_LOGGED: usize = 16;

/// The stick must be held at least this far for its speed to count.
const MIN_STICK: f32 = 0.2;

/// How long a hold settles before its root speed is sampled, and then sampled for.
const SETTLE: Duration = Duration::from_secs(1);
const MEASURE: Duration = Duration::from_millis(1500);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_speed_is_measured_once() {
        let mut log = SpeedLog::new();
        let start = Instant::now();
        let mut measured = vec![];
        for tick in 0..300u64 {
            let now = start + Duration::from_millis(tick * 33);
            let root = if tick < 30 { 1.0 } else { 4.4 };
            if let Some(m) = log.fold(now, 1.0, Some(4.5), root) {
                measured.push(m);
            }
        }
        assert_eq!(measured.len(), 1);
        assert_eq!(measured[0].target_mps, 4.5);
        assert!((measured[0].root_mps - 4.4).abs() < 1e-4);
    }

    #[test]
    fn releasing_the_stick_restarts_the_hold() {
        let mut log = SpeedLog::new();
        let start = Instant::now();
        for tick in 0..60u64 {
            let now = start + Duration::from_millis(tick * 33);
            let stick = if tick % 20 == 19 { 0.0 } else { 1.0 };
            assert_eq!(log.fold(now, stick, Some(1.6), 1.6), None);
        }
    }
}
