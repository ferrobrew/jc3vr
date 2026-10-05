//! The seated eye reference: the head height above the STAGE floor that the seated posture puts at
//! the character's standing eye height. Pure.
//!
//! A single calibrated height is fragile. It belongs to one chair, one way of sitting, and one
//! moment, and a player who sits 15 cm lower than they did while calibrating sees the world from
//! 15 cm below the character's eyes for the whole session, in a vehicle seat as much as on foot.
//! The reference therefore follows the player:
//!
//! - it starts at the player's calibrated seated eye height, or, without one, at the head's
//!   height on the first seated frame, and a new calibration starts it again;
//! - a manual recenter while seated captures the head's height at that moment ([`capture`]);
//! - the adaptive baseline moves it toward the median head height over a long window, at a rate
//!   too slow to notice, so that a reference that does not match how the player sits settles onto
//!   how they do.
//!
//! Deliberate head movement stays one to one (scaled by the height ratio): a lean, a duck, or a
//! look over the shoulder held for less than half the window does not move the median, and the
//! reference never moves faster than the configured rate.
//!
//! [`capture`]: SeatedReference::capture

use crate::controllers::config::SeatedReferenceConfig;

/// The seated eye reference in effect, and where it came from.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SeatedEyeReference {
    /// The head's height above the STAGE floor that maps to the character's standing eye height,
    /// metres.
    pub height_m: f32,
    pub source: SeatedReferenceSource,
}

/// Where the seated eye reference came from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SeatedReferenceSource {
    /// The player's calibrated seated eye height.
    Calibrated,
    /// The head's height on the first seated frame, without a calibration.
    FirstSample,
    /// The head's height at a manual recenter while seated.
    Recenter,
    /// The adaptive baseline has moved it toward the median head height.
    Adaptive,
}

impl SeatedReferenceSource {
    pub fn label(self) -> &'static str {
        match self {
            SeatedReferenceSource::Calibrated => "calibrated",
            SeatedReferenceSource::FirstSample => "first sample",
            SeatedReferenceSource::Recenter => "recenter",
            SeatedReferenceSource::Adaptive => "adaptive",
        }
    }
}

/// The adaptive baseline's state, for the readouts.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SeatedReferenceStatus {
    pub reference: Option<SeatedEyeReference>,
    /// The median head height over the window, once the window holds enough samples to adapt from.
    pub median_m: Option<f32>,
    /// The samples in the window.
    pub samples: usize,
}

/// Tracks the seated eye reference from frame to frame.
#[derive(Debug, Clone, PartialEq)]
pub struct SeatedReference {
    reference: Option<SeatedEyeReference>,
    /// The valid calibrated height seen on the last update, `None` before the first update, so
    /// that a new calibration restarts the reference and the first one seeds it.
    calibration: Option<Option<f32>>,
    samples: SampleRing,
    since_sample_s: f32,
    /// Whether the adaptive baseline is closing a gap, which it does until the gap is well inside
    /// the deadband, so that the reference does not hover at the deadband's edge.
    adapting: bool,
    median_m: Option<f32>,
}

impl SeatedReference {
    pub const fn new() -> Self {
        Self {
            reference: None,
            calibration: None,
            samples: SampleRing::new(),
            since_sample_s: 0.0,
            adapting: false,
            median_m: None,
        }
    }

    /// Advance by one seated frame: seed the reference if it has none, sample `head_height` (the
    /// head above the STAGE floor, when located), and let the adaptive baseline move it. `calibrated`
    /// is the player's calibrated seated eye height; a change to it restarts the reference from it. `dt` is
    /// the time since the last frame, seconds.
    pub fn update(
        &mut self,
        head_height: Option<f32>,
        calibrated: Option<f32>,
        dt: f32,
        config: &SeatedReferenceConfig,
    ) -> Option<SeatedEyeReference> {
        let calibrated = calibrated.filter(|h| plausible(*h));
        let previous = self.calibration.replace(calibrated);
        if previous != Some(calibrated)
            && let Some(height_m) = calibrated
            && (previous.is_some() || self.reference.is_none())
        {
            self.restart(height_m, SeatedReferenceSource::Calibrated);
        }

        let head = head_height.filter(|h| plausible(*h));
        if self.reference.is_none()
            && let Some(height_m) = head
        {
            self.restart(height_m, SeatedReferenceSource::FirstSample);
        }

        if !config.adaptive {
            self.forget_samples();
            return self.reference;
        }
        let dt = step(dt);
        if let Some(height_m) = head {
            self.since_sample_s += dt;
            let interval = sample_interval(config.window_s);
            if self.since_sample_s >= interval {
                self.since_sample_s = (self.since_sample_s - interval).min(interval);
                self.samples.push(height_m);
                self.median_m = (self.samples.len() >= MIN_SAMPLES)
                    .then(|| self.samples.median())
                    .flatten();
            }
        }
        self.adapt(dt, config);
        self.reference
    }

    /// Make `head_height` (the head above the STAGE floor at a manual recenter) the reference, and
    /// start the adaptive window again, so that samples from before the recenter do not pull the
    /// reference back. Returns whether the height was usable.
    pub fn capture(&mut self, head_height: f32) -> bool {
        if !plausible(head_height) {
            return false;
        }
        self.restart(head_height, SeatedReferenceSource::Recenter);
        true
    }

    /// The reference in effect, without advancing.
    pub fn current(&self) -> Option<SeatedEyeReference> {
        self.reference
    }

    pub fn status(&self) -> SeatedReferenceStatus {
        SeatedReferenceStatus {
            reference: self.reference,
            median_m: self.median_m,
            samples: self.samples.len(),
        }
    }

    fn restart(&mut self, height_m: f32, source: SeatedReferenceSource) {
        self.reference = Some(SeatedEyeReference { height_m, source });
        self.forget_samples();
    }

    fn forget_samples(&mut self) {
        self.samples.clear();
        self.since_sample_s = 0.0;
        self.adapting = false;
        self.median_m = None;
    }

    fn adapt(&mut self, dt: f32, config: &SeatedReferenceConfig) {
        let (Some(reference), Some(median)) = (self.reference.as_mut(), self.median_m) else {
            return;
        };
        let gap = median - reference.height_m;
        let deadband = config.deadband_m.max(0.0);
        if gap.abs() > deadband {
            self.adapting = true;
        } else if gap.abs() <= deadband * SETTLE_FRACTION {
            self.adapting = false;
        }
        if !self.adapting {
            return;
        }
        let limit = config.rate_mps.max(0.0) * dt;
        let move_m = gap.clamp(-limit, limit);
        if move_m != 0.0 {
            reference.height_m += move_m;
            reference.source = SeatedReferenceSource::Adaptive;
        }
    }
}

impl Default for SeatedReference {
    fn default() -> Self {
        Self::new()
    }
}

/// The samples the window holds; the sampling interval is the window divided by this.
const CAPACITY: usize = 240;

/// The samples the window must hold before the median moves the reference: a quarter of the window.
const MIN_SAMPLES: usize = CAPACITY / 4;

/// Once adapting, the adaptive baseline closes the gap to this fraction of the deadband.
const SETTLE_FRACTION: f32 = 0.25;

/// The longest frame step the tracker accounts for, seconds, so that a stall does not count as
/// seated time.
const MAX_STEP_S: f32 = 0.25;

/// The range a head height above the floor must fall in to be used, metres.
const PLAUSIBLE_HEIGHT_M: (f32, f32) = (0.3, 2.5);

fn plausible(height_m: f32) -> bool {
    height_m.is_finite() && (PLAUSIBLE_HEIGHT_M.0..=PLAUSIBLE_HEIGHT_M.1).contains(&height_m)
}

fn step(dt: f32) -> f32 {
    if dt.is_finite() {
        dt.clamp(0.0, MAX_STEP_S)
    } else {
        0.0
    }
}

fn sample_interval(window_s: f32) -> f32 {
    let window_s = if window_s.is_finite() {
        window_s.max(MIN_WINDOW_S)
    } else {
        SeatedReferenceConfig::new().window_s
    };
    window_s / CAPACITY as f32
}

/// The shortest window the adaptive baseline takes, seconds.
const MIN_WINDOW_S: f32 = 10.0;

/// A fixed-capacity ring of head heights, oldest overwritten first.
#[derive(Debug, Clone, PartialEq)]
struct SampleRing {
    values: [f32; CAPACITY],
    len: usize,
    next: usize,
}

impl SampleRing {
    const fn new() -> Self {
        Self {
            values: [0.0; CAPACITY],
            len: 0,
            next: 0,
        }
    }

    fn push(&mut self, value: f32) {
        self.values[self.next] = value;
        self.next = (self.next + 1) % CAPACITY;
        self.len = (self.len + 1).min(CAPACITY);
    }

    fn len(&self) -> usize {
        self.len
    }

    fn clear(&mut self) {
        self.len = 0;
        self.next = 0;
    }

    fn median(&self) -> Option<f32> {
        if self.len == 0 {
            return None;
        }
        let mut sorted = self.values;
        let sorted = &mut sorted[..self.len];
        sorted.sort_unstable_by(f32::total_cmp);
        let mid = self.len / 2;
        Some(if self.len.is_multiple_of(2) {
            0.5 * (sorted[mid - 1] + sorted[mid])
        } else {
            sorted[mid]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;
    /// The calibrated seated eye height of the session the fix was made from.
    const CALIBRATED: f32 = 1.218;
    /// Where that player's head actually sat through the session.
    const SITTING: f32 = 1.07;
    const FRAME_S: f32 = 1.0 / 90.0;

    fn config() -> SeatedReferenceConfig {
        SeatedReferenceConfig::new()
    }

    /// Run `seconds` of frames with the head at `head(t)`.
    fn run(
        reference: &mut SeatedReference,
        seconds: f32,
        calibrated: Option<f32>,
        head: impl Fn(f32) -> f32,
    ) -> SeatedEyeReference {
        let frames = (seconds / FRAME_S).round() as usize;
        let mut last = None;
        for i in 0..frames {
            last = reference.update(
                Some(head(i as f32 * FRAME_S)),
                calibrated,
                FRAME_S,
                &config(),
            );
        }
        last.unwrap()
    }

    #[test]
    fn the_calibration_seeds_the_reference() {
        let mut r = SeatedReference::new();
        let seeded = r.update(Some(SITTING), Some(CALIBRATED), 0.0, &config());
        assert_eq!(
            seeded,
            Some(SeatedEyeReference {
                height_m: CALIBRATED,
                source: SeatedReferenceSource::Calibrated
            })
        );
    }

    #[test]
    fn without_a_calibration_the_first_located_head_seeds_it() {
        let mut r = SeatedReference::new();
        assert_eq!(r.update(None, None, FRAME_S, &config()), None);
        let seeded = r.update(Some(SITTING), None, FRAME_S, &config()).unwrap();
        assert_eq!(seeded.source, SeatedReferenceSource::FirstSample);
        assert!((seeded.height_m - SITTING).abs() < EPS);
    }

    #[test]
    fn a_recenter_captures_the_head_and_survives_the_calibration() {
        let mut r = SeatedReference::new();
        r.update(Some(SITTING), Some(CALIBRATED), FRAME_S, &config());
        assert!(r.capture(SITTING));
        let after = r
            .update(Some(SITTING), Some(CALIBRATED), FRAME_S, &config())
            .unwrap();
        assert_eq!(after.source, SeatedReferenceSource::Recenter);
        assert!((after.height_m - SITTING).abs() < EPS);
        assert!(!r.capture(f32::NAN));
        assert!(!r.capture(-1.0));
    }

    #[test]
    fn a_capture_before_the_first_frame_is_not_overwritten_by_the_calibration() {
        let mut r = SeatedReference::new();
        assert!(r.capture(SITTING));
        let after = r
            .update(Some(SITTING), Some(CALIBRATED), FRAME_S, &config())
            .unwrap();
        assert_eq!(after.source, SeatedReferenceSource::Recenter);
    }

    #[test]
    fn a_new_calibration_restarts_the_reference() {
        let mut r = SeatedReference::new();
        r.update(Some(SITTING), Some(CALIBRATED), FRAME_S, &config());
        r.capture(SITTING);
        let recalibrated = r
            .update(Some(SITTING), Some(1.10), FRAME_S, &config())
            .unwrap();
        assert_eq!(recalibrated.source, SeatedReferenceSource::Calibrated);
        assert!((recalibrated.height_m - 1.10).abs() < EPS);
    }

    /// The logged session: calibrated at 1.22 m, sitting at 1.03 to 1.10 m. The adaptive baseline
    /// brings the reference onto the head within a minute and a half.
    #[test]
    fn the_adaptive_baseline_settles_onto_how_the_player_sits() {
        let mut r = SeatedReference::new();
        let logged = [
            1.057, 1.049, 1.009, 1.089, 1.079, 1.073, 1.067, 1.052, 1.074, 1.046,
        ];
        let settled = run(&mut r, 90.0, Some(CALIBRATED), |t| {
            logged[(t / 5.0) as usize % logged.len()]
        });
        let median = r.status().median_m.unwrap();
        assert_eq!(settled.source, SeatedReferenceSource::Adaptive);
        assert!(
            (settled.height_m - median).abs() <= config().deadband_m,
            "settled at {} against a median of {median}",
            settled.height_m
        );
        assert!((median - 1.062).abs() < 0.02, "median {median}");
    }

    #[test]
    fn the_adaptive_baseline_moves_no_faster_than_its_rate() {
        let mut r = SeatedReference::new();
        // Fill the window, then watch one second.
        let before = run(&mut r, 30.0, Some(CALIBRATED), |_| SITTING).height_m;
        let after = run(&mut r, 1.0, Some(CALIBRATED), |_| SITTING).height_m;
        assert!(before - after <= config().rate_mps + EPS);
        assert!(before > after, "it moves toward the head");
    }

    #[test]
    fn a_brief_duck_or_lean_does_not_move_the_reference() {
        let mut r = SeatedReference::new();
        r.capture(SITTING);
        // Two minutes seated, with a 20 s duck and a 15 s lean forward in it.
        let held = run(&mut r, 120.0, None, |t| match t {
            t if (30.0..50.0).contains(&t) => SITTING - 0.35,
            t if (80.0..95.0).contains(&t) => SITTING - 0.12,
            _ => SITTING,
        });
        assert_eq!(held.source, SeatedReferenceSource::Recenter);
        assert!((held.height_m - SITTING).abs() < EPS);
    }

    #[test]
    fn a_capture_starts_the_window_again() {
        let mut r = SeatedReference::new();
        run(&mut r, 60.0, None, |_| 1.20);
        assert!(r.status().samples > 0);
        r.capture(SITTING);
        assert_eq!(r.status().samples, 0);
        assert_eq!(r.status().median_m, None);
    }

    #[test]
    fn with_the_adaptive_baseline_off_the_reference_holds() {
        let off = SeatedReferenceConfig {
            adaptive: false,
            ..config()
        };
        let mut r = SeatedReference::new();
        for _ in 0..(120.0 / FRAME_S) as usize {
            r.update(Some(SITTING), Some(CALIBRATED), FRAME_S, &off);
        }
        let held = r.current().unwrap();
        assert_eq!(held.source, SeatedReferenceSource::Calibrated);
        assert_eq!(r.status().samples, 0);
    }

    #[test]
    fn implausible_heights_are_ignored() {
        let mut r = SeatedReference::new();
        assert_eq!(
            r.update(Some(f32::NAN), Some(9.0), FRAME_S, &config()),
            None
        );
        assert_eq!(r.update(Some(-0.2), None, FRAME_S, &config()), None);
    }

    #[test]
    fn a_stall_does_not_count_as_seated_time() {
        let mut r = SeatedReference::new();
        r.capture(1.20);
        r.update(Some(SITTING), None, 3600.0, &config());
        assert!(r.status().samples <= 1);
        assert_eq!(r.current().unwrap().height_m, 1.20);
    }

    #[test]
    fn the_median_takes_the_middle_of_an_even_window() {
        let mut ring = SampleRing::new();
        assert_eq!(ring.median(), None);
        for v in [4.0, 1.0, 3.0, 2.0] {
            ring.push(v);
        }
        assert_eq!(ring.median(), Some(2.5));
        for v in 0..CAPACITY + 5 {
            ring.push(v as f32);
        }
        assert_eq!(ring.len(), CAPACITY);
    }
}
