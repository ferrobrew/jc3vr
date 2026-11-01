//! The grapple hand's pull on the line, measured from its reach along its aim when the pull armed.
//! Pure.
//!
//! The pull arms when the reel starts or the tether retract arms ([`LinePull`]), and the aim the
//! hand has then is the line it pulls back along ([`reach_along`]): a reel follows a fire or a tap
//! along the line the hand pointed, and the retract follows a hold of the button at what the
//! tethers hold. A pull back from where the hand pointed counts whichever way it goes, so a seated
//! player's hand, which starts close to the chest and pulls back past its side, pulls; its distance
//! from the body hardly changes over such a pull, and its reach forward of the head takes only the
//! part of it that goes straight back.
//!
//! A hand on a rope pulls in two ways, and the tracker measures both from one reach sample per
//! input tick:
//!
//! - the pull's speed: how fast the hand comes back along the line, peak-held and decaying over
//!   [`PullTuning::speed_hold_s`], so that one sharp pull is still felt across a reel that lasts
//!   longer than the arm's reach;
//! - the pull's depth: how far the hand has come back from a reference that jumps out with the hand
//!   and relaxes toward a hand held still at [`PullTuning::relax_mps`]. Reaching out and pulling
//!   back deepens the pull; holding the hand back lets it fade; a hand-over-hand pull keeps it up.

use glam::Vec3;

use crate::{
    controllers::bindings::yank::{HandLine, reach_along},
    grapple::physical::config::PullTuning,
};

/// A pull measured along the aim the hand had when it armed.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LinePull {
    /// The aim at the arming, once a tracked hand gave one; cleared while disarmed.
    line: Option<Vec3>,
    tracker: PullTracker,
}

impl LinePull {
    pub const fn new() -> Self {
        Self {
            line: None,
            tracker: PullTracker::new(),
        }
    }

    /// Advance by `dt` seconds with the hand's grip and aim (`None` while untracked); `armed` is
    /// whether the reel or the retract reads the pull. The first tracked tick of an arming keeps
    /// its aim as the line and starts the pull from the hand there. Disarmed, the pull reads
    /// nothing.
    pub fn step(
        &mut self,
        dt: f32,
        hand: Option<HandLine>,
        armed: bool,
        config: &PullTuning,
    ) -> PullReading {
        if !armed {
            *self = Self::new();
            return PullReading::default();
        }
        if self.line.is_none() {
            self.line = hand.map(|hand| hand.aim).filter(|aim| aim.is_finite());
            self.tracker = PullTracker::new();
        }
        let reach = self
            .line
            .zip(hand)
            .and_then(|(line, hand)| reach_along(hand.grip, line));
        self.tracker.step(dt, reach, config)
    }

    /// Start the pull's depth from the hand where it is ([`PullTracker::rebase`]).
    pub fn rebase(&mut self) {
        self.tracker.rebase();
    }
}

/// One tick's reading of the pull.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PullReading {
    /// The peak-held speed of the hand back along the line, metres per second, never negative.
    pub speed_mps: f32,
    /// How far the hand is back from the reference, metres, never negative.
    pub depth_m: f32,
}

/// The pull tracker's state.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PullTracker {
    /// The last tick's reach along the line, metres.
    previous: Option<f32>,
    /// The depth's reference reach, metres.
    reference: Option<f32>,
    speed: f32,
}

impl PullTracker {
    pub const fn new() -> Self {
        Self {
            previous: None,
            reference: None,
            speed: 0.0,
        }
    }

    /// Advance by `dt` seconds with the hand's reach along the line, or `None` while the hand is
    /// untracked, which forgets the pull.
    pub fn step(&mut self, dt: f32, distance: Option<f32>, config: &PullTuning) -> PullReading {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let Some(distance) = distance.filter(|d| d.is_finite()) else {
            *self = Self::new();
            return PullReading::default();
        };
        let decay = if config.speed_hold_s > f32::EPSILON {
            (-dt / config.speed_hold_s).exp()
        } else {
            0.0
        };
        let instant = match self.previous {
            Some(previous) if dt > f32::EPSILON => ((previous - distance) / dt).max(0.0),
            _ => 0.0,
        };
        self.speed = (self.speed * decay).max(instant);
        self.previous = Some(distance);
        let reference = self.reference.map_or(distance, |r| {
            (r - config.relax_mps.max(0.0) * dt).max(distance)
        });
        self.reference = Some(reference);
        PullReading {
            speed_mps: self.speed,
            depth_m: reference - distance,
        }
    }

    /// Move the depth's reference to the hand, so that the depth starts from zero: the pull
    /// arms for a new use (a tether that just formed) without a hand already back reading as a
    /// pull.
    pub fn rebase(&mut self) {
        self.reference = self.previous;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grapple::physical::config::GRAPPLE;

    const DT: f32 = 1.0 / 30.0;

    fn run(tracker: &mut PullTracker, distances: &[f32]) -> Vec<PullReading> {
        let config = GRAPPLE.pull;
        distances
            .iter()
            .map(|&d| tracker.step(DT, Some(d), &config))
            .collect()
    }

    #[test]
    fn a_pull_reads_its_speed_and_depth() {
        let mut t = PullTracker::new();
        let readings = run(&mut t, &[0.6, 0.55, 0.5, 0.45]);
        let last = readings.last().unwrap();
        assert!((last.speed_mps - 1.5).abs() < 1e-3, "{last:?}");
        assert!(last.depth_m > 0.12 && last.depth_m <= 0.15, "{last:?}");
    }

    #[test]
    fn a_push_away_does_not_pull() {
        let mut t = PullTracker::new();
        let readings = run(&mut t, &[0.3, 0.4, 0.5, 0.6]);
        assert!(
            readings
                .iter()
                .all(|r| r.speed_mps == 0.0 && r.depth_m == 0.0)
        );
    }

    #[test]
    fn the_speed_is_held_and_decays() {
        let mut t = PullTracker::new();
        run(&mut t, &[0.6, 0.5]);
        let held = run(&mut t, &[0.5; 6]);
        assert!(held[0].speed_mps > 2.5);
        assert!(held.windows(2).all(|w| w[1].speed_mps < w[0].speed_mps));
        let later = run(&mut t, &[0.5; 120]);
        assert!(later.last().unwrap().speed_mps < 0.05);
    }

    #[test]
    fn a_hand_held_back_lets_the_depth_fade() {
        let mut t = PullTracker::new();
        run(&mut t, &[0.6, 0.5, 0.4, 0.3]);
        let held = run(&mut t, &[0.3; 60]);
        assert!(held[0].depth_m > 0.25);
        assert_eq!(held.last().unwrap().depth_m, 0.0);
    }

    #[test]
    fn reaching_out_moves_the_reference_with_the_hand() {
        let mut t = PullTracker::new();
        run(&mut t, &[0.3, 0.6]);
        let back = run(&mut t, &[0.5]);
        assert!((back[0].depth_m - 0.1).abs() < 0.01, "{back:?}");
    }

    /// A seated player's grapple hand, the line out ahead and to the right, pulled back and out past
    /// their left side at the game's tick, with its aim at the arming and the head; cockpit frame.
    /// The shape of a recorded seated pull.
    const SEATED_PULL: [Vec3; 7] = [
        Vec3::new(-0.029, -0.274, -0.390),
        Vec3::new(-0.060, -0.252, -0.381),
        Vec3::new(-0.101, -0.235, -0.354),
        Vec3::new(-0.150, -0.221, -0.329),
        Vec3::new(-0.191, -0.216, -0.307),
        Vec3::new(-0.216, -0.216, -0.288),
        Vec3::new(-0.225, -0.219, -0.277),
    ];
    const SEATED_AIM: Vec3 = Vec3::new(0.51, -0.46, -0.73);
    const SEATED_HEAD: Vec3 = Vec3::new(0.07, -0.046, 0.0);
    const SEATED_DT: f32 = 0.053;

    fn peak(readings: impl Iterator<Item = PullReading>) -> PullReading {
        readings.fold(PullReading::default(), |peak, r| PullReading {
            speed_mps: peak.speed_mps.max(r.speed_mps),
            depth_m: peak.depth_m.max(r.depth_m),
        })
    }

    #[test]
    fn a_seated_pull_past_the_side_pulls_along_the_armed_aim() {
        let config = GRAPPLE.pull;
        let mut pull = LinePull::new();
        let along = peak(SEATED_PULL.iter().map(|&grip| {
            pull.step(
                SEATED_DT,
                Some(HandLine {
                    grip,
                    aim: SEATED_AIM,
                }),
                true,
                &config,
            )
        }));
        assert!(along.speed_mps > 0.7, "{along:?}");
        assert!(along.depth_m > GRAPPLE.tether.deadzone_m, "{along:?}");
        // The same pull hardly nears a point below the head.
        let below_head = SEATED_HEAD - Vec3::Y * 0.3;
        let mut tracker = PullTracker::new();
        let body = peak(
            SEATED_PULL
                .iter()
                .map(|&grip| tracker.step(SEATED_DT, Some(grip.distance(below_head)), &config)),
        );
        assert!(body.depth_m < 0.03, "{body:?}");
    }

    #[test]
    fn a_disarmed_pull_reads_nothing_and_rearms_from_the_hand() {
        let config = GRAPPLE.pull;
        let mut pull = LinePull::new();
        let hand = |z: f32| {
            Some(HandLine {
                grip: Vec3::new(0.0, 0.0, z),
                aim: Vec3::NEG_Z,
            })
        };
        pull.step(DT, hand(-0.6), true, &config);
        assert!(pull.step(DT, hand(-0.4), true, &config).depth_m > 0.15);
        assert_eq!(
            pull.step(DT, hand(-0.3), false, &config),
            PullReading::default()
        );
        // Re-armed with the hand already back, the pull starts from there.
        assert_eq!(
            pull.step(DT, hand(-0.3), true, &config),
            PullReading::default()
        );
    }

    #[test]
    fn a_rebase_zeroes_the_depth() {
        let mut t = PullTracker::new();
        run(&mut t, &[0.6, 0.4]);
        t.rebase();
        let after = run(&mut t, &[0.4]);
        assert_eq!(after[0].depth_m, 0.0);
    }

    #[test]
    fn an_untracked_hand_forgets_the_pull() {
        let mut t = PullTracker::new();
        run(&mut t, &[0.6, 0.4]);
        let config = GRAPPLE.pull;
        assert_eq!(t.step(DT, None, &config), PullReading::default());
        let resumed = run(&mut t, &[0.2]);
        assert_eq!(resumed[0], PullReading::default());
        assert_eq!(t.step(DT, Some(f32::NAN), &config), PullReading::default());
    }
}
