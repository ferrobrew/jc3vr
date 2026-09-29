//! The yank gesture: a sharp pull of the grapple hand back toward the body while a grapple line is
//! attached, which issues the same grapple tap that zips (the plan of record's "The grapple",
//! `docs/mod/input/controllers-and-roomscale.md`). Pure.
//!
//! The detector keeps the grapple hand's distance from the body over a short window, sampled at
//! the input tick. A yank is a drop in that distance of at least the configured length, from the
//! farthest sample in the window to the current one, covered at the configured speed or faster.
//! After a yank the history is cleared and a cooldown runs, so one pull issues one tap.

use std::collections::VecDeque;

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// The yank gesture's settings, held in
/// [`BindingsConfig`](crate::controllers::config::BindingsConfig). Acts only under the controller
/// scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct YankConfig {
    pub enabled: bool,
    /// The shortest pull toward the body that counts, metres.
    pub min_distance_m: f32,
    /// The slowest average speed of the pull that counts, metres per second.
    pub min_speed_mps: f32,
    /// How far back the pull is measured from, seconds.
    pub window_s: f32,
    /// How long after a yank before another can count, seconds.
    pub cooldown_s: f32,
    /// How far below the head the body point the hand is pulled toward sits, metres.
    pub body_drop_m: f32,
}

impl YankConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            min_distance_m: 0.15,
            min_speed_mps: 1.8,
            window_s: 0.2,
            cooldown_s: 0.6,
            body_drop_m: 0.3,
        }
    }
}

impl Default for YankConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The body point a yank pulls toward: below the head, in the cockpit frame.
pub fn body_point(head: Vec3, config: &YankConfig) -> Vec3 {
    head - Vec3::Y * config.body_drop_m
}

/// The yank detector's history.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct YankDetector {
    /// `(time, distance)` samples within the window, oldest first.
    history: VecDeque<(f32, f32)>,
    time: f32,
    cooldown: f32,
    /// The pull and its speed at the last yank, for the debug UI.
    pub last: Option<Yank>,
    pub count: u64,
}

/// A detected yank.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Yank {
    /// How far the hand came toward the body, metres.
    pub distance_m: f32,
    /// The pull's average speed, metres per second.
    pub speed_mps: f32,
}

impl YankDetector {
    pub const fn new() -> Self {
        Self {
            history: VecDeque::new(),
            time: 0.0,
            cooldown: 0.0,
            last: None,
            count: 0,
        }
    }

    /// Advance by `dt` seconds with the hand's current distance from the body (`None` while the
    /// hand is untracked). `armed` is whether a yank may fire: a line is attached and the gesture
    /// is on. Returns the yank detected this tick, if any.
    pub fn step(
        &mut self,
        dt: f32,
        distance: Option<f32>,
        armed: bool,
        config: &YankConfig,
    ) -> Option<Yank> {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.time += dt;
        self.cooldown = (self.cooldown - dt).max(0.0);
        let Some(distance) = distance.filter(|d| d.is_finite() && armed && config.enabled) else {
            self.history.clear();
            return None;
        };
        self.history.push_back((self.time, distance));
        while self
            .history
            .front()
            .is_some_and(|&(t, _)| self.time - t > config.window_s)
        {
            self.history.pop_front();
        }
        if self.cooldown > 0.0 {
            return None;
        }
        let &(t0, farthest) = self.history.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
        let pulled = farthest - distance;
        let elapsed = self.time - t0;
        if elapsed <= 0.0 || pulled < config.min_distance_m {
            return None;
        }
        let speed = pulled / elapsed;
        if speed < config.min_speed_mps {
            return None;
        }
        let yank = Yank {
            distance_m: pulled,
            speed_mps: speed,
        };
        self.history.clear();
        self.cooldown = config.cooldown_s;
        self.last = Some(yank);
        self.count += 1;
        Some(yank)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn run(detector: &mut YankDetector, distances: &[f32], armed: bool) -> Vec<bool> {
        let config = YankConfig::new();
        distances
            .iter()
            .map(|&d| detector.step(DT, Some(d), armed, &config).is_some())
            .collect()
    }

    #[test]
    fn a_sharp_pull_yanks_once() {
        let mut d = YankDetector::new();
        let fired = run(&mut d, &[0.6, 0.6, 0.5, 0.38, 0.3, 0.29, 0.29], true);
        assert_eq!(fired.iter().filter(|f| **f).count(), 1);
        assert_eq!(d.count, 1);
        assert!(d.last.unwrap().distance_m >= 0.15);
    }

    #[test]
    fn a_slow_pull_does_not_yank() {
        let mut d = YankDetector::new();
        let slow: Vec<f32> = (0..30).map(|i| 0.6 - i as f32 * 0.01).collect();
        assert!(run(&mut d, &slow, true).iter().all(|f| !f));
    }

    #[test]
    fn a_short_twitch_does_not_yank() {
        let mut d = YankDetector::new();
        assert!(run(&mut d, &[0.6, 0.52, 0.5, 0.6], true).iter().all(|f| !f));
    }

    #[test]
    fn a_push_away_does_not_yank() {
        let mut d = YankDetector::new();
        assert!(run(&mut d, &[0.3, 0.45, 0.6, 0.7], true).iter().all(|f| !f));
    }

    #[test]
    fn nothing_yanks_without_a_line() {
        let mut d = YankDetector::new();
        assert!(run(&mut d, &[0.6, 0.45, 0.3], false).iter().all(|f| !f));
    }

    #[test]
    fn the_cooldown_holds_off_a_second_yank() {
        let mut d = YankDetector::new();
        run(&mut d, &[0.6, 0.45, 0.3], true);
        assert_eq!(d.count, 1);
        run(&mut d, &[0.6, 0.45, 0.3], true);
        assert_eq!(d.count, 1);
        let rest = vec![0.6; 20];
        run(&mut d, &rest, true);
        run(&mut d, &[0.6, 0.45, 0.3], true);
        assert_eq!(d.count, 2);
    }
}
