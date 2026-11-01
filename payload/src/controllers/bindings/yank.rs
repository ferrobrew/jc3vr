//! The yank gesture: a sharp pull of the grapple hand back toward the body while a grapple line is
//! attached, which issues the same grapple tap that zips (the plan of record's "The grapple",
//! `docs/mod/input/controllers-and-roomscale.md`). Pure.
//!
//! The detector keeps a measure of the grapple hand's reach over a short window, sampled at the
//! input tick. A yank is a drop in that measure of at least the configured length, from any sample
//! in the window to the current one, covered at the configured speed or faster. After a yank the
//! history is cleared and a cooldown runs, so one pull issues one tap.
//!
//! The measure is the hand's reach along its aim when the gesture armed ([`reach_along`], kept by
//! [`LineYank`] for the zip and the reel's boost): a pull away from what the hand pointed at,
//! whichever way it goes. The hand's distance from a point below the head would miss the pull of a
//! seated player, whose hand starts close to the body and pulls back past its side.

use std::collections::VecDeque;

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// The grapple yank's switch, held in
/// [`BindingsConfig`](crate::controllers::config::BindingsConfig): a sharp pull of the grapple hand
/// toward the body zips along an attached line. Acts only under the controller scheme, so it
/// defaults on. Its detector is tuned at [`GRAPPLE_YANK`].
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrappleYankConfig {
    pub enabled: bool,
}

impl GrappleYankConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }

    /// The detector's settings: [`GRAPPLE_YANK`] with this switch.
    pub const fn detector(&self) -> YankConfig {
        YankConfig {
            enabled: self.enabled,
            ..GRAPPLE_YANK
        }
    }
}

impl Default for GrappleYankConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The grapple yank's detector settings, with the switch on.
pub const GRAPPLE_YANK: YankConfig = YankConfig {
    enabled: true,
    min_distance_m: 0.15,
    min_speed_mps: 1.8,
    window_s: 0.2,
    cooldown_s: 0.6,
};

/// A yank detector's settings: the grapple's ([`GRAPPLE_YANK`]), or the snatch's, held in
/// [`SnatchConfig`](crate::controllers::snatch::config::SnatchConfig).
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
}

/// The grapple hand's grip position and aim direction, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandLine {
    pub grip: Vec3,
    pub aim: Vec3,
}

/// How far along `line`, the grapple hand's aim when its gesture armed, the hand's grip at `grip`
/// reaches, metres; both in the cockpit frame. A yank back from what the hand pointed at is a drop
/// in it, whichever way the line points and wherever the hand is held. `None` for a line with no
/// direction.
pub fn reach_along(grip: Vec3, line: Vec3) -> Option<f32> {
    let line = line.try_normalize()?;
    let reach = grip.dot(line);
    reach.is_finite().then_some(reach)
}

/// A yank measured along the aim the hand had when the gesture armed: the zip's, armed while a line
/// is attached and the grapple button is held.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LineYank {
    /// The aim at the arming, once a tracked hand gave one; cleared while disarmed.
    line: Option<Vec3>,
    pub detector: YankDetector,
}

impl LineYank {
    pub const fn new() -> Self {
        Self {
            line: None,
            detector: YankDetector::new(),
        }
    }

    /// Advance by `dt` seconds with the hand's grip and aim (`None` while untracked); `armed` is
    /// whether a yank may fire. The first tracked tick of an arming keeps its aim as the line.
    /// Returns the yank detected this tick, if any.
    pub fn step(
        &mut self,
        dt: f32,
        hand: Option<HandLine>,
        armed: bool,
        config: &YankConfig,
    ) -> Option<Yank> {
        if !armed {
            self.line = None;
        } else if self.line.is_none() {
            self.line = hand.map(|hand| hand.aim).filter(|aim| aim.is_finite());
        }
        let reach = self
            .line
            .zip(hand)
            .and_then(|(line, hand)| reach_along(hand.grip, line));
        self.detector.step(dt, reach, armed, config)
    }
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
        // Every sample in the window may start the pull, so a hand held still before it pulls
        // does not slow the pull's measured speed with the pause.
        let yank = self
            .history
            .iter()
            .filter_map(|&(t, from)| {
                let pulled = from - distance;
                let elapsed = self.time - t;
                (elapsed > 0.0 && pulled >= config.min_distance_m).then(|| Yank {
                    distance_m: pulled,
                    speed_mps: pulled / elapsed,
                })
            })
            .filter(|yank| yank.speed_mps >= config.min_speed_mps)
            .max_by(|a, b| a.distance_m.total_cmp(&b.distance_m))?;
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
        let config = GRAPPLE_YANK;
        distances
            .iter()
            .map(|&d| detector.step(DT, Some(d), armed, &config).is_some())
            .collect()
    }

    /// A seated player's grapple hand, the line out ahead and to the right, pulled sharply back and
    /// out past their left side at the game's tick, with its aim at the arming and the head;
    /// cockpit frame. The shape of a recorded seated pull, quickened to a zip's speed.
    const SEATED_ZIP: [Vec3; 5] = [
        Vec3::new(-0.029, -0.274, -0.390),
        Vec3::new(-0.029, -0.274, -0.390),
        Vec3::new(-0.101, -0.235, -0.354),
        Vec3::new(-0.173, -0.217, -0.317),
        Vec3::new(-0.225, -0.219, -0.277),
    ];
    const SEATED_AIM: Vec3 = Vec3::new(0.51, -0.46, -0.73);
    const SEATED_HEAD: Vec3 = Vec3::new(0.07, -0.046, 0.0);
    const SEATED_DT: f32 = 0.0266;

    fn zip_hand(grip: Vec3, aim: Vec3) -> Option<HandLine> {
        Some(HandLine { grip, aim })
    }

    #[test]
    fn a_seated_zip_pull_past_the_side_yanks_along_the_armed_aim() {
        let mut zip = LineYank::new();
        // The hand's aim turns with the wrist during the pull; the line stays the arming's.
        let fired = SEATED_ZIP
            .iter()
            .enumerate()
            .filter_map(|(k, &grip)| {
                let aim = SEATED_AIM + Vec3::new(-0.1, 0.1, 0.0) * k as f32;
                zip.step(SEATED_DT, zip_hand(grip, aim), true, &GRAPPLE_YANK)
            })
            .count();
        assert_eq!(fired, 1);
        // The body point's distance barely changes over the same pull.
        let body = SEATED_HEAD - Vec3::Y * 0.3;
        let mut detector = YankDetector::new();
        assert!(SEATED_ZIP.iter().all(|grip| {
            detector
                .step(SEATED_DT, Some(grip.distance(body)), true, &GRAPPLE_YANK)
                .is_none()
        }));
    }

    #[test]
    fn the_zip_line_is_taken_afresh_at_each_arming() {
        let mut zip = LineYank::new();
        // Armed aiming down -Z: a pull along +X is across the line and no yank.
        for k in 0..6 {
            let grip = Vec3::new(0.06 * k as f32, -0.25, -0.4);
            assert!(
                zip.step(SEATED_DT, zip_hand(grip, Vec3::NEG_Z), true, &GRAPPLE_YANK)
                    .is_none()
            );
        }
        zip.step(
            SEATED_DT,
            zip_hand(Vec3::ZERO, Vec3::NEG_Z),
            false,
            &GRAPPLE_YANK,
        );
        // Re-armed aiming down -X, the same pull along +X is back from the line, and yanks.
        let fired = (0..6)
            .filter(|&k| {
                let grip = Vec3::new(0.06 * k as f32, -0.25, -0.4);
                zip.step(SEATED_DT, zip_hand(grip, Vec3::NEG_X), true, &GRAPPLE_YANK)
                    .is_some()
            })
            .count();
        assert_eq!(fired, 1);
    }

    #[test]
    fn an_untracked_arming_takes_its_line_from_the_first_tracked_tick() {
        let mut zip = LineYank::new();
        assert!(zip.step(SEATED_DT, None, true, &GRAPPLE_YANK).is_none());
        let fired = SEATED_ZIP
            .iter()
            .filter(|&&grip| {
                zip.step(SEATED_DT, zip_hand(grip, SEATED_AIM), true, &GRAPPLE_YANK)
                    .is_some()
            })
            .count();
        assert_eq!(fired, 1);
    }

    #[test]
    fn a_line_with_no_direction_measures_nothing() {
        assert_eq!(reach_along(Vec3::ONE, Vec3::ZERO), None);
        assert_eq!(reach_along(Vec3::ONE, Vec3::NAN), None);
        assert_eq!(
            reach_along(Vec3::new(0.0, 0.0, -0.4), Vec3::NEG_Z),
            Some(0.4)
        );
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
    fn a_pause_before_the_pull_does_not_slow_it() {
        let mut d = YankDetector::new();
        // Still for most of the window, then 0.16 m in two ticks.
        let fired = run(&mut d, &[0.6, 0.6, 0.6, 0.6, 0.52, 0.44], true);
        assert_eq!(fired, [false, false, false, false, false, true]);
        assert!(d.last.unwrap().speed_mps >= 2.0);
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
