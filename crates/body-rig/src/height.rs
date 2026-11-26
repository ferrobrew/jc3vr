//! The height the player is seen to stand at.
//!
//! The calibrated eye height and the runtime's floor disagree by a few centimetres from session to
//! session, and near vertical a few centimetres of drop are a large lean, so the standing neck
//! height is measured rather than assumed: the height nine in ten of the recent level-gaze samples
//! lie below, seeded at the calibration and held within [`MAX_DRIFT_M`] of it, so that half a
//! minute crouched behind cover never becomes the standing height.

/// How often a sample is taken, seconds.
pub const SAMPLE_S: f32 = 0.125;

/// How many samples are held: half a minute of them.
pub const WINDOW: usize = 240;

/// How many samples are needed before they replace the seed.
pub const MIN_SAMPLES: usize = 16;

/// The furthest the measure may stray from the seed, metres.
pub const MAX_DRIFT_M: f32 = 0.15;

/// The share of the samples the standing height is above.
pub const PERCENTILE: f32 = 0.9;

/// The standing neck height.
#[derive(Debug, Clone, PartialEq)]
pub struct StandingHeight {
    seed: f32,
    /// A ring of the latest samples, the oldest at `next` once full.
    samples: [f32; WINDOW],
    count: usize,
    next: usize,
    since_sample: f32,
    reference: f32,
}

impl StandingHeight {
    /// A measure seeded at `seed`, metres over the floor.
    pub const fn new(seed: f32) -> Self {
        Self {
            seed,
            samples: [0.0; WINDOW],
            count: 0,
            next: 0,
            since_sample: 0.0,
            reference: seed,
        }
    }

    /// The seed the measure is held near.
    pub fn seed(&self) -> f32 {
        self.seed
    }

    /// The standing neck height, metres over the floor.
    pub fn reference(&self) -> f32 {
        self.reference
    }

    /// How many samples are held.
    pub fn samples(&self) -> usize {
        self.count
    }

    /// Start over from `seed`.
    pub fn reseed(&mut self, seed: f32) {
        *self = Self::new(seed);
    }

    /// Start over from where the neck stands now, held within the drift of the current seed: a
    /// recenter, taken standing upright.
    pub fn restart_at(&mut self, height: f32) {
        let seed = self.seed;
        let height = if height.is_finite() {
            height.clamp(seed - MAX_DRIFT_M, seed + MAX_DRIFT_M)
        } else {
            seed
        };
        *self = Self::new(seed);
        self.reference = height;
        self.push(height);
    }

    fn push(&mut self, height: f32) {
        self.samples[self.next] = height;
        self.next = (self.next + 1) % WINDOW;
        self.count = (self.count + 1).min(WINDOW);
    }

    /// Advance by `dt` seconds with the neck at `height`, taking a sample when `sample`.
    pub fn observe(&mut self, height: f32, sample: bool, dt: f32) -> f32 {
        if dt.is_finite() && dt > 0.0 {
            self.since_sample += dt;
        }
        if sample && height.is_finite() && self.since_sample >= SAMPLE_S {
            self.since_sample = 0.0;
            self.push(height);
            if self.count >= MIN_SAMPLES {
                let mut sorted = self.samples;
                let sorted = &mut sorted[..self.count];
                sorted.sort_by(f32::total_cmp);
                let index = ((sorted.len() - 1) as f32 * PERCENTILE).round() as usize;
                self.reference =
                    sorted[index].clamp(self.seed - MAX_DRIFT_M, self.seed + MAX_DRIFT_M);
            }
        }
        self.reference
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_settles_on_the_upper_samples() {
        let mut height = StandingHeight::new(1.6);
        for i in 0..400 {
            // A standing head that bobs between 1.55 and 1.57.
            let h = 1.55 + 0.02 * ((i % 10) as f32 / 9.0);
            height.observe(h, true, SAMPLE_S);
        }
        assert!(
            (height.reference() - 1.568).abs() < 0.005,
            "{}",
            height.reference()
        );
    }

    #[test]
    fn a_long_crouch_stays_within_the_drift() {
        let mut height = StandingHeight::new(1.6);
        for _ in 0..400 {
            height.observe(1.1, true, SAMPLE_S);
        }
        assert!((height.reference() - (1.6 - MAX_DRIFT_M)).abs() < 1e-5);
    }

    #[test]
    fn unsampled_frames_leave_it() {
        let mut height = StandingHeight::new(1.6);
        for _ in 0..400 {
            height.observe(1.0, false, SAMPLE_S);
        }
        assert_eq!(height.reference(), 1.6);
    }

    #[test]
    fn a_restart_takes_the_current_height() {
        let mut height = StandingHeight::new(1.6);
        height.restart_at(1.55);
        assert_eq!(height.reference(), 1.55);
        height.restart_at(1.0);
        assert_eq!(height.reference(), 1.6 - MAX_DRIFT_M);
    }
}
