//! The yank gesture: a sharp pull of the grapple hand back toward the body while a grapple line is
//! attached, which issues the same grapple tap that zips (the plan of record's "The grapple",
//! `docs/mod/input/controllers-and-roomscale.md`). Pure.
//!
//! The detector keeps a measure of the grapple hand's reach over a short window, sampled at the
//! input tick. A yank is a drop in that measure of at least the configured length, from any sample
//! in the window to the current one, covered at the configured speed or faster. After a yank the
//! history is cleared and a cooldown runs, so one pull issues one tap.
//!
//! The measure is the hand's reach along its aim when the gesture armed ([`reach_along`], latched
//! by [`ArmedLine`], which [`LineYank`] keeps for the zip and the reel's boost): a pull away from
//! what the hand pointed at, whichever way it goes. The hand's distance from a point below the head would miss the pull of a
//! seated player, whose hand starts close to the body and pulls back past its side.
//!
//! A pull that carries the hand out from the head across the ground is not a yank
//! (the controls tuning's `yank.outward_share`): spreading the arms for the wingsuit swings the grapple hand out to the side,
//! which drops its reach along an aim pointed across the body as fast as a yank, but takes it away
//! from the body, where a yank brings it in, down, or back past the side. The distance is taken
//! across the ground alone, so a pull down from a raised hand, which takes it further from the head
//! but not out from it, still yanks.

use std::collections::VecDeque;

use glam::Vec3;
use jc3vr_core::config::engine::{Invalid, nonnegative, positive};
use serde::Deserialize;

use crate::controllers::intent::tuning::CONTROLS;

/// The grapple yank's detector settings (the controls tuning's `grapple_yank`): a sharp pull of
/// the grapple hand toward the body zips along an attached line. Acts only under the controller
/// scheme.
pub fn grapple_yank() -> &'static YankConfig {
    &CONTROLS.get().grapple_yank
}

/// A yank detector's settings: the grapple's ([`grapple_yank`]), or the snatch's (`snatch_yank` in
/// the grapple crate's snatch).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YankConfig {
    /// The shortest pull toward the body that counts, metres.
    pub min_distance_m: f32,
    /// The slowest average speed of the pull that counts, metres per second.
    pub min_speed_mps: f32,
    /// How far back the pull is measured from, seconds.
    pub window_s: f32,
    /// How long after a yank before another can count, seconds.
    pub cooldown_s: f32,
}

impl YankConfig {
    /// Check the settings as the tuning file at `name` holds them.
    pub fn validate(&self, name: &str) -> Result<(), Invalid> {
        positive(&format!("{name}.min_distance_m"), self.min_distance_m, 5.0)?;
        positive(&format!("{name}.min_speed_mps"), self.min_speed_mps, 50.0)?;
        positive(&format!("{name}.window_s"), self.window_s, 10.0)?;
        nonnegative(&format!("{name}.cooldown_s"), self.cooldown_s, 60.0)
    }
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

/// The aim the grapple hand had when a gesture armed: the line each armed hand gesture measures the
/// hand's reach along ([`reach_along`]). The zip's yank and the reel's boost ([`LineYank`]), the
/// reel's and the retract's pull, the lock's take-in, and the snatch's yank each keep one. The
/// gestures' detectors differ; only this latch is shared.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ArmedLine {
    /// The aim at the arming, once a tracked hand gave a finite one.
    line: Option<Vec3>,
}

impl ArmedLine {
    pub const fn new() -> Self {
        Self { line: None }
    }

    /// The reach along the line of the hand at `hand` (`None` while untracked), metres. The first
    /// tracked tick with a finite aim latches that aim as the line; before it, and while the hand
    /// is untracked, there is no reach.
    pub fn reach(&mut self, hand: Option<HandLine>) -> Option<f32> {
        if self.line.is_none() {
            self.line = hand.map(|hand| hand.aim).filter(|aim| aim.is_finite());
        }
        self.line
            .zip(hand)
            .and_then(|(line, hand)| reach_along(hand.grip, line))
    }

    /// Whether the line is latched.
    pub fn latched(&self) -> bool {
        self.line.is_some()
    }

    /// Forget the line, so that the next arming latches its own.
    pub fn disarm(&mut self) {
        self.line = None;
    }
}

/// A yank measured along the aim the hand had when the gesture armed: the zip's, armed while a line
/// is attached and the grapple button is held.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LineYank {
    /// The aim at the arming; disarmed while the yank is.
    line: ArmedLine,
    pub detector: YankDetector,
}

impl LineYank {
    pub const fn new() -> Self {
        Self {
            line: ArmedLine::new(),
            detector: YankDetector::new(),
        }
    }

    /// Advance by `dt` seconds with the hand's grip and aim (`None` while untracked); `armed` is
    /// whether a yank may fire. The first tracked tick of an arming keeps its aim as the line.
    /// Returns the yank detected this tick, if any. The head is not considered: the tests' measure
    /// of the reach alone.
    #[cfg(test)]
    pub fn step(
        &mut self,
        dt: f32,
        hand: Option<HandLine>,
        armed: bool,
        config: &YankConfig,
    ) -> Option<Yank> {
        self.step_from(dt, hand, None, armed, config)
    }

    /// [`step`](Self::step), with the head's position in the same frame as the hand, whose Y is up
    /// (`None` when unknown): a pull that takes the hand out from the head across the ground does
    /// not yank.
    pub fn step_from(
        &mut self,
        dt: f32,
        hand: Option<HandLine>,
        head: Option<Vec3>,
        armed: bool,
        config: &YankConfig,
    ) -> Option<Yank> {
        let reach = if armed {
            self.line.reach(hand)
        } else {
            self.line.disarm();
            None
        };
        let outward = hand
            .zip(head)
            .map(|(hand, head)| (hand.grip - head).with_y(0.0).length())
            .filter(|d| d.is_finite());
        self.detector
            .step_outward(dt, reach, outward, armed, config)
    }
}

/// The yank detector's history.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct YankDetector {
    /// `(time, distance, distance from the head)` samples within the window, oldest first.
    history: VecDeque<(f32, f32, Option<f32>)>,
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
        self.step_outward(dt, distance, None, armed, config)
    }

    /// [`step`](Self::step), with the hand's distance from the head across the ground (`None` when
    /// unknown): a pull that takes the hand out from the head by more than the controls tuning's
    /// `yank.outward_share` of the pull does not yank.
    pub fn step_outward(
        &mut self,
        dt: f32,
        distance: Option<f32>,
        outward: Option<f32>,
        armed: bool,
        config: &YankConfig,
    ) -> Option<Yank> {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.time += dt;
        self.cooldown = (self.cooldown - dt).max(0.0);
        let Some(distance) = distance.filter(|d| d.is_finite() && armed) else {
            self.history.clear();
            return None;
        };
        self.history.push_back((self.time, distance, outward));
        while self
            .history
            .front()
            .is_some_and(|&(t, _, _)| self.time - t > config.window_s)
        {
            self.history.pop_front();
        }
        if self.cooldown > 0.0 {
            return None;
        }
        let outward_share = CONTROLS.get().yank.outward_share;
        // Every sample in the window may start the pull, so a hand held still before it pulls
        // does not slow the pull's measured speed with the pause.
        let yank = self
            .history
            .iter()
            .filter_map(|&(t, from, from_head)| {
                let pulled = from - distance;
                let elapsed = self.time - t;
                let away = from_head
                    .zip(outward)
                    .map_or(0.0, |(from_head, head)| head - from_head);
                (elapsed > 0.0 && pulled >= config.min_distance_m && away <= outward_share * pulled)
                    .then(|| Yank {
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
    use body_rig::math::constants::SEATED_TICK_S;

    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn run(detector: &mut YankDetector, distances: &[f32], armed: bool) -> Vec<bool> {
        let config = *grapple_yank();
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

    fn zip_hand(grip: Vec3, aim: Vec3) -> Option<HandLine> {
        Some(HandLine { grip, aim })
    }

    #[test]
    fn an_arm_spread_out_to_the_side_does_not_yank() {
        // The 10 November take, 1492.6 s: the grapple hand, aimed ahead and across the body, swung
        // out to the side to open the wingsuit; its reach along the aim dropped 0.36 m in 0.2 s,
        // and it went 0.3 m further out from the head across the ground.
        let head = Vec3::new(0.0, 0.0, 0.0);
        let aim = Vec3::new(0.62, -0.17, -0.77);
        let from = Vec3::new(-0.05, -0.35, -0.35);
        let spread = Vec3::new(-0.53, 0.14, 0.01);
        let mut zip = LineYank::new();
        let fired = (0..6)
            .filter(|&k| {
                let grip = from + spread * (k as f32 / 5.0);
                zip.step_from(DT, zip_hand(grip, aim), Some(head), true, grapple_yank())
                    .is_some()
            })
            .count();
        assert_eq!(fired, 0);
        // Without the head, the same motion reads as a yank.
        let mut blind = LineYank::new();
        let fired = (0..6)
            .filter(|&k| {
                let grip = from + spread * (k as f32 / 5.0);
                blind
                    .step(DT, zip_hand(grip, aim), true, grapple_yank())
                    .is_some()
            })
            .count();
        assert_eq!(fired, 1);
    }

    #[test]
    fn a_pull_down_from_a_raised_hand_still_yanks() {
        // A line up to a high point: the hand, raised in front of the face, yanks down to the
        // chest, further from the head but no further out from it.
        let head = Vec3::ZERO;
        let aim = Vec3::new(0.0, 0.8, -0.6);
        let from = Vec3::new(-0.1, 0.1, -0.45);
        let to = Vec3::new(-0.15, -0.45, -0.3);
        let mut zip = LineYank::new();
        let fired = (0..6)
            .filter(|&k| {
                let grip = from.lerp(to, k as f32 / 5.0);
                zip.step_from(DT, zip_hand(grip, aim), Some(head), true, grapple_yank())
                    .is_some()
            })
            .count();
        assert_eq!(fired, 1);
    }

    #[test]
    fn a_seated_pull_past_the_side_still_yanks_with_the_head_known() {
        let mut zip = LineYank::new();
        let fired = SEATED_ZIP
            .iter()
            .filter(|&&grip| {
                zip.step_from(
                    SEATED_TICK_S,
                    zip_hand(grip, SEATED_AIM),
                    Some(SEATED_HEAD),
                    true,
                    grapple_yank(),
                )
                .is_some()
            })
            .count();
        assert_eq!(fired, 1);
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
                zip.step(SEATED_TICK_S, zip_hand(grip, aim), true, grapple_yank())
            })
            .count();
        assert_eq!(fired, 1);
        // The body point's distance barely changes over the same pull.
        let body = SEATED_HEAD - Vec3::Y * 0.3;
        let mut detector = YankDetector::new();
        assert!(SEATED_ZIP.iter().all(|grip| {
            detector
                .step(
                    SEATED_TICK_S,
                    Some(grip.distance(body)),
                    true,
                    grapple_yank(),
                )
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
                zip.step(
                    SEATED_TICK_S,
                    zip_hand(grip, Vec3::NEG_Z),
                    true,
                    grapple_yank()
                )
                .is_none()
            );
        }
        zip.step(
            SEATED_TICK_S,
            zip_hand(Vec3::ZERO, Vec3::NEG_Z),
            false,
            grapple_yank(),
        );
        // Re-armed aiming down -X, the same pull along +X is back from the line, and yanks.
        let fired = (0..6)
            .filter(|&k| {
                let grip = Vec3::new(0.06 * k as f32, -0.25, -0.4);
                zip.step(
                    SEATED_TICK_S,
                    zip_hand(grip, Vec3::NEG_X),
                    true,
                    grapple_yank(),
                )
                .is_some()
            })
            .count();
        assert_eq!(fired, 1);
    }

    #[test]
    fn an_untracked_arming_takes_its_line_from_the_first_tracked_tick() {
        let mut zip = LineYank::new();
        assert!(
            zip.step(SEATED_TICK_S, None, true, grapple_yank())
                .is_none()
        );
        let fired = SEATED_ZIP
            .iter()
            .filter(|&&grip| {
                zip.step(
                    SEATED_TICK_S,
                    zip_hand(grip, SEATED_AIM),
                    true,
                    grapple_yank(),
                )
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
    fn the_armed_line_latches_the_first_finite_aim_until_disarmed() {
        let hand = |aim: Vec3| {
            Some(HandLine {
                grip: Vec3::new(0.0, 0.0, -0.4),
                aim,
            })
        };
        let mut line = ArmedLine::new();
        assert_eq!(line.reach(None), None);
        assert_eq!(line.reach(hand(Vec3::NAN)), None);
        assert!(!line.latched(), "a non-finite aim latches nothing");
        assert_eq!(line.reach(hand(Vec3::NEG_Z)), Some(0.4));
        assert_eq!(
            line.reach(hand(Vec3::X)),
            Some(0.4),
            "the latched aim holds"
        );
        assert_eq!(line.reach(None), None);
        line.disarm();
        assert_eq!(line.reach(hand(Vec3::X)), Some(0.0));
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
