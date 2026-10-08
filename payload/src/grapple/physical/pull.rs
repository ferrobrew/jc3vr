//! The grapple hand's pull on the line, measured from its distance to the body. Pure.
//!
//! A hand on a rope pulls in two ways, and the tracker measures both from one distance sample per
//! input tick:
//!
//! - the pull's speed: how fast the hand comes toward the body, peak-held and decaying over
//!   [`PullConfig::speed_hold_s`], so that one sharp pull is still felt across a reel that lasts
//!   longer than the arm's reach;
//! - the pull's depth: how far the hand has come back from a reference that jumps out with the hand
//!   and relaxes toward a hand held still at [`PullConfig::relax_mps`]. Reaching out and pulling
//!   back deepens the pull; holding the hand back lets it fade; a hand-over-hand pull keeps it up.

use crate::grapple::physical::config::PullConfig;

/// One tick's reading of the pull.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PullReading {
    /// The peak-held speed of the hand toward the body, metres per second, never negative.
    pub speed_mps: f32,
    /// How far the hand is back from the reference, metres, never negative.
    pub depth_m: f32,
}

/// The pull tracker's state.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PullTracker {
    /// The last tick's distance from the body, metres.
    previous: Option<f32>,
    /// The depth's reference distance, metres.
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

    /// Advance by `dt` seconds with the hand's distance from the body, or `None` while the hand is
    /// untracked, which forgets the pull.
    pub fn step(&mut self, dt: f32, distance: Option<f32>, config: &PullConfig) -> PullReading {
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

    const DT: f32 = 1.0 / 30.0;

    fn run(tracker: &mut PullTracker, distances: &[f32]) -> Vec<PullReading> {
        let config = PullConfig::new();
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
        let config = PullConfig::new();
        assert_eq!(t.step(DT, None, &config), PullReading::default());
        let resumed = run(&mut t, &[0.2]);
        assert_eq!(resumed[0], PullReading::default());
        assert_eq!(t.step(DT, Some(f32::NAN), &config), PullReading::default());
    }
}
