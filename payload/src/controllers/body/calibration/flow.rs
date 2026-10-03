//! The calibration flow: its steps, the capture that samples a live measurement, and what went
//! wrong.

use std::time::{Duration, Instant};

use crate::controllers::{
    body::{
        BodyDimensions,
        calibration::{CalibrationError, Measurements, check_eye_height, derive},
    },
    frame::Hand,
};

/// How long the flow waits after MEASURE is selected, so that the player can take the pose.
pub const COUNTDOWN: Duration = Duration::from_secs(3);
/// How long the flow samples once the countdown ends.
pub const SAMPLE_WINDOW: Duration = Duration::from_secs(1);
/// The fewest samples a capture needs to count.
pub const MIN_SAMPLES: u32 = 5;

/// Why a live measurement is not available.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// No VR session is rendering.
    NoHeadset,
    /// The session has no STAGE space, so the floor is unknown.
    NoFloor,
    /// The STAGE space could not be located this frame.
    HeadNotTracked,
    /// No controller frame is being published (the controllers are off or absent).
    NoControllers,
    /// This hand's controller has no position this frame, measured or inferred.
    ControllerLost(Hand),
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unavailable::NoHeadset => write!(f, "no VR frame is rendering"),
            Unavailable::NoFloor => write!(f, "the session has no STAGE space"),
            Unavailable::HeadNotTracked => write!(f, "the STAGE space cannot be located"),
            Unavailable::NoControllers => write!(f, "no controller frame is published"),
            Unavailable::ControllerLost(Hand::Left) => {
                write!(f, "the left controller has no position")
            }
            Unavailable::ControllerLost(Hand::Right) => {
                write!(f, "the right controller has no position")
            }
        }
    }
}

/// A step of the calibration flow.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Step {
    /// Not calibrating: the page shows the saved dimensions.
    Summary,
    /// Standing straight, to measure the eye height.
    Stand,
    /// The T-pose, to measure the arm span.
    Span,
    /// Reviewing the derived dimensions before saving.
    Review,
}

/// The last thing that went wrong, shown until the next attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// MEASURE was selected while the measurement was unavailable.
    Refused(Unavailable),
    /// The sampling window ended with too few samples; `cause` is why the last sample was missed,
    /// when one was.
    TooFewSamples { cause: Option<Unavailable> },
    /// The measurement cannot describe a body.
    Rejected(CalibrationError),
    /// Saving the body file failed.
    SaveFailed(String),
}

impl std::fmt::Display for Notice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Notice::Refused(unavailable) => {
                write!(f, "body calibration: measuring refused: {unavailable}")
            }
            Notice::TooFewSamples { cause } => {
                write!(
                    f,
                    "body calibration: fewer than {MIN_SAMPLES} samples in the window"
                )?;
                match cause {
                    Some(cause) => write!(f, ": {cause}"),
                    None => Ok(()),
                }
            }
            Notice::Rejected(error) => write!(f, "{error}"),
            Notice::SaveFailed(message) => write!(f, "body calibration: {message}"),
        }
    }
}

/// What the capture is doing, for the status readout.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CaptureStatus {
    /// No capture is running.
    Idle,
    /// Counting down to the sampling window; whole seconds left, rounded up.
    Countdown(u32),
    /// Sampling.
    Sampling,
}

/// The calibration flow's state.
#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    step: Step,
    eye_height: Option<f32>,
    grip_span: Option<f32>,
    capture: Capture,
    notice: Option<Notice>,
    /// How many notices have been raised since the flow was created, kept across resets so that a
    /// reader can tell a new notice from one it has seen.
    notices: u32,
}

impl Calibration {
    /// Not calibrating.
    pub const fn new() -> Self {
        Self {
            step: Step::Summary,
            eye_height: None,
            grip_span: None,
            capture: Capture::Idle,
            notice: None,
            notices: 0,
        }
    }

    /// The current step.
    pub fn step(&self) -> Step {
        self.step
    }

    /// The last problem, if the flow has one to show.
    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    /// How many notices have been raised; it changes whenever one is, even when it equals the last.
    pub fn notice_count(&self) -> u32 {
        self.notices
    }

    /// The measured eye height, once the standing step is done.
    pub fn eye_height(&self) -> Option<f32> {
        self.eye_height
    }

    /// What the capture is doing at `now`.
    pub fn capture_status(&self, now: Instant) -> CaptureStatus {
        match self.capture {
            Capture::Idle => CaptureStatus::Idle,
            Capture::Countdown { ends } => {
                let left = ends.saturating_duration_since(now);
                CaptureStatus::Countdown(left.as_secs_f32().ceil() as u32)
            }
            Capture::Sampling { .. } => CaptureStatus::Sampling,
        }
    }

    /// The dimensions derived from both measurements, once both are taken.
    pub fn result(&self) -> Option<Result<BodyDimensions, CalibrationError>> {
        Some(derive(Measurements {
            eye_height: self.eye_height?,
            grip_span: self.grip_span?,
        }))
    }

    /// Begin calibrating, from the summary. Returns whether the step changed.
    pub fn start(&mut self) -> bool {
        if self.step != Step::Summary {
            return false;
        }
        self.reset(Step::Stand);
        true
    }

    /// Start a capture on a measuring step, given the live measurement at `now`. Refuses, with a
    /// notice, when the measurement is unavailable. Returns whether a capture started.
    pub fn measure(&mut self, now: Instant, live: Result<f32, Unavailable>) -> bool {
        if !matches!(self.step, Step::Stand | Step::Span) || self.capture != Capture::Idle {
            return false;
        }
        if let Err(unavailable) = live {
            self.raise(Notice::Refused(unavailable));
            return false;
        }
        self.notice = None;
        self.capture = Capture::Countdown {
            ends: now + COUNTDOWN,
        };
        true
    }

    /// Advance the capture to `now` with the live measurement. An unavailable measurement is
    /// skipped, during the countdown (the player is moving into the pose) and during sampling (a
    /// controller out of sight for a frame); the window then counts only the samples it got. A
    /// finished capture moves to the next step when the value is plausible. Returns whether the
    /// step changed.
    pub fn tick(&mut self, now: Instant, live: Result<f32, Unavailable>) -> bool {
        match &mut self.capture {
            Capture::Idle => false,
            Capture::Countdown { ends } => {
                if now >= *ends {
                    self.capture = Capture::Sampling {
                        ends: now + SAMPLE_WINDOW,
                        samples: Samples::default(),
                    };
                }
                false
            }
            Capture::Sampling { ends, samples } => {
                match live {
                    Ok(value) if value.is_finite() => samples.add(value),
                    Ok(_) => {}
                    Err(unavailable) => samples.missed = Some(unavailable),
                }
                if now < *ends {
                    return false;
                }
                let samples = *samples;
                self.capture = Capture::Idle;
                self.finish(samples)
            }
        }
    }

    /// Go back one step: from the T-pose to the standing step, from the review to the T-pose
    /// (keeping the height), or from the standing step to the summary. Returns whether the step
    /// changed.
    pub fn back(&mut self) -> bool {
        match self.step {
            Step::Summary => false,
            Step::Stand => self.cancel(),
            Step::Span => {
                self.step = Step::Stand;
                self.eye_height = None;
                self.capture = Capture::Idle;
                self.notice = None;
                true
            }
            Step::Review => {
                self.step = Step::Span;
                self.grip_span = None;
                self.notice = None;
                true
            }
        }
    }

    /// Measure again from the standing step, from the review. Returns whether the step changed.
    pub fn retry(&mut self) -> bool {
        if self.step != Step::Review {
            return false;
        }
        self.reset(Step::Stand);
        true
    }

    /// Abandon the calibration and return to the summary. Returns whether the step changed.
    pub fn cancel(&mut self) -> bool {
        let changed = self.step != Step::Summary;
        self.reset(Step::Summary);
        changed
    }

    /// Record that the result was saved: the flow returns to the summary.
    pub fn saved(&mut self) {
        self.reset(Step::Summary);
    }

    /// Record that saving the result failed; the flow stays on the review.
    pub fn save_failed(&mut self, message: String) {
        self.raise(Notice::SaveFailed(message));
    }

    /// Clear everything but the notice count and go to `step`.
    fn reset(&mut self, step: Step) {
        *self = Self {
            step,
            notices: self.notices,
            ..Self::new()
        };
    }

    fn raise(&mut self, notice: Notice) {
        self.notice = Some(notice);
        self.notices = self.notices.wrapping_add(1);
    }

    /// Take a finished sampling window's value and move on if it is plausible.
    fn finish(&mut self, samples: Samples) -> bool {
        if samples.count < MIN_SAMPLES {
            self.raise(Notice::TooFewSamples {
                cause: samples.missed,
            });
            return false;
        }
        match self.step {
            Step::Stand => {
                // Standing still: the mean rides out the head's sway.
                let eye_height = samples.mean();
                if let Err(error) = check_eye_height(eye_height) {
                    self.raise(Notice::Rejected(error));
                    return false;
                }
                self.eye_height = Some(eye_height);
                self.notice = None;
                self.step = Step::Span;
                true
            }
            Step::Span => {
                // A T-pose is at its widest when the arms are fully out.
                self.grip_span = Some(samples.max);
                match self.result() {
                    Some(Ok(_)) => {
                        self.notice = None;
                        self.step = Step::Review;
                        true
                    }
                    Some(Err(error)) => {
                        self.grip_span = None;
                        self.raise(Notice::Rejected(error));
                        false
                    }
                    None => false,
                }
            }
            Step::Summary | Step::Review => false,
        }
    }
}

impl Default for Calibration {
    fn default() -> Self {
        Self::new()
    }
}

/// The capture in progress.
#[derive(Debug, Copy, Clone, PartialEq)]
enum Capture {
    Idle,
    Countdown { ends: Instant },
    Sampling { ends: Instant, samples: Samples },
}

/// The samples of one sampling window.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Samples {
    count: u32,
    sum: f32,
    max: f32,
    /// Why the last missed sample was missed.
    missed: Option<Unavailable>,
}

impl Samples {
    fn add(&mut self, value: f32) {
        self.max = if self.count == 0 {
            value
        } else {
            self.max.max(value)
        };
        self.count += 1;
        self.sum += value;
    }

    fn mean(&self) -> f32 {
        self.sum / self.count as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::calibration::{
        arm_span_from_grip_span, height_from_eye_height, span_out_of_proportion,
        tests::{SEATED_EYE_HEIGHT, player},
    };

    /// Run a capture from MEASURE to the end of its sampling window with a constant value.
    fn capture(calibration: &mut Calibration, t0: Instant, value: f32) -> bool {
        assert!(calibration.measure(t0, Ok(value)));
        let sampling = t0 + COUNTDOWN;
        assert!(!calibration.tick(sampling, Ok(value)));
        assert_eq!(
            calibration.capture_status(sampling),
            CaptureStatus::Sampling
        );
        let mut changed = false;
        for i in 1..=10 {
            let now = sampling + SAMPLE_WINDOW * i / 10;
            changed |= calibration.tick(now, Ok(value));
        }
        changed
    }

    #[test]
    fn a_full_calibration() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        assert_eq!(calibration.step(), Step::Summary);
        assert!(calibration.result().is_none());
        assert!(calibration.start());
        assert_eq!(calibration.step(), Step::Stand);

        assert!(capture(&mut calibration, t0, player().eye_height));
        assert_eq!(calibration.step(), Step::Span);
        assert!((calibration.eye_height().unwrap() - player().eye_height).abs() < 1e-5);

        assert!(capture(&mut calibration, t0, player().grip_span));
        assert_eq!(calibration.step(), Step::Review);
        let body = calibration.result().unwrap().unwrap();
        assert_eq!(body, derive(player()).unwrap());

        calibration.saved();
        assert_eq!(calibration.step(), Step::Summary);
        assert!(calibration.result().is_none());
    }

    #[test]
    fn the_countdown_counts_whole_seconds() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start();
        assert_eq!(calibration.capture_status(t0), CaptureStatus::Idle);
        calibration.measure(t0, Ok(1.6));
        assert_eq!(calibration.capture_status(t0), CaptureStatus::Countdown(3));
        assert_eq!(
            calibration.capture_status(t0 + Duration::from_millis(2500)),
            CaptureStatus::Countdown(1)
        );
        // A lost measurement during the countdown is tolerated.
        assert!(!calibration.tick(
            t0 + Duration::from_secs(1),
            Err(Unavailable::HeadNotTracked)
        ));
        assert!(calibration.notice().is_none());
    }

    #[test]
    fn measure_refuses_without_a_measurement() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start();
        assert!(!calibration.measure(t0, Err(Unavailable::NoFloor)));
        assert_eq!(
            calibration.notice(),
            Some(&Notice::Refused(Unavailable::NoFloor))
        );
        assert_eq!(calibration.capture_status(t0), CaptureStatus::Idle);
        // The next successful MEASURE clears the notice.
        assert!(calibration.measure(t0, Ok(1.6)));
        assert!(calibration.notice().is_none());
        // A second MEASURE while capturing does nothing.
        assert!(!calibration.measure(t0, Ok(1.6)));
    }

    #[test]
    fn measure_does_nothing_outside_the_measuring_steps() {
        let mut calibration = Calibration::new();
        assert!(!calibration.measure(Instant::now(), Ok(1.6)));
        assert_eq!(
            calibration.capture_status(Instant::now()),
            CaptureStatus::Idle
        );
    }

    #[test]
    fn missed_samples_are_skipped() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start();
        calibration.measure(t0, Ok(1.6));
        let start = t0 + COUNTDOWN;
        calibration.tick(start, Ok(1.6));
        let lost = Unavailable::HeadNotTracked;
        for i in 1..=10 {
            let live = if i % 2 == 0 { Err(lost) } else { Ok(1.6) };
            let now = start + SAMPLE_WINDOW * i / 10;
            assert_eq!(calibration.tick(now, live), i == 10);
        }
        assert_eq!(calibration.step(), Step::Span);
        assert!(calibration.notice().is_none());
    }

    #[test]
    fn a_window_of_missed_samples_says_why() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start();
        capture(&mut calibration, t0, player().eye_height);
        calibration.measure(t0, Ok(1.6));
        let start = t0 + COUNTDOWN;
        calibration.tick(start, Ok(1.6));
        let lost = Unavailable::ControllerLost(Hand::Left);
        for i in 1..=10 {
            assert!(!calibration.tick(start + SAMPLE_WINDOW * i / 10, Err(lost)));
        }
        assert_eq!(
            calibration.notice(),
            Some(&Notice::TooFewSamples { cause: Some(lost) })
        );
        assert_eq!(calibration.step(), Step::Span);
        assert_eq!(calibration.capture_status(t0), CaptureStatus::Idle);
    }

    #[test]
    fn too_few_samples_do_not_count() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start();
        calibration.measure(t0, Ok(1.6));
        calibration.tick(t0 + COUNTDOWN, Ok(1.6));
        assert!(!calibration.tick(t0 + COUNTDOWN + SAMPLE_WINDOW, Ok(1.6)));
        assert_eq!(
            calibration.notice(),
            Some(&Notice::TooFewSamples { cause: None })
        );
        assert_eq!(calibration.step(), Step::Stand);
    }

    #[test]
    fn an_implausible_capture_stays_on_its_step() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start();
        assert!(!capture(&mut calibration, t0, 0.8));
        assert_eq!(calibration.step(), Step::Stand);
        assert!(matches!(
            calibration.notice(),
            Some(Notice::Rejected(CalibrationError::EyeHeightTooLow(_)))
        ));

        assert!(capture(&mut calibration, t0, player().eye_height));
        assert!(!capture(&mut calibration, t0, 0.4));
        assert_eq!(calibration.step(), Step::Span);
        assert!(matches!(
            calibration.notice(),
            Some(Notice::Rejected(CalibrationError::ArmsNotSpread { .. }))
        ));
        assert!(calibration.result().is_none());
    }

    #[test]
    fn a_seated_calibration_reaches_the_review() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start();
        assert!(capture(&mut calibration, t0, SEATED_EYE_HEIGHT));
        assert!(capture(&mut calibration, t0, player().grip_span));
        assert_eq!(calibration.step(), Step::Review);
        assert!(calibration.notice().is_none());
        assert!(span_out_of_proportion(
            &calibration.result().unwrap().unwrap()
        ));
    }

    #[test]
    fn every_notice_is_counted() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start();
        assert_eq!(calibration.notice_count(), 0);
        calibration.measure(t0, Err(Unavailable::NoFloor));
        calibration.measure(t0, Err(Unavailable::NoFloor));
        assert_eq!(calibration.notice_count(), 2);
        // A success clears the notice without counting.
        calibration.measure(t0, Ok(1.6));
        assert!(calibration.notice().is_none());
        assert_eq!(calibration.notice_count(), 2);
        // The count survives leaving the flow, so a reader never sees it go backwards.
        calibration.cancel();
        assert_eq!(calibration.notice_count(), 2);
        calibration.start();
        calibration.measure(t0, Err(Unavailable::NoHeadset));
        assert_eq!(calibration.notice_count(), 3);
    }

    #[test]
    fn the_span_takes_the_widest_sample_and_the_height_the_mean() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start();
        calibration.measure(t0, Ok(1.0));
        let start = t0 + COUNTDOWN;
        calibration.tick(start, Ok(1.66));
        for (i, value) in [1.66, 1.70, 1.66, 1.70, 1.66, 1.70].into_iter().enumerate() {
            calibration.tick(start + Duration::from_millis(100 * i as u64), Ok(value));
        }
        assert!(calibration.tick(start + SAMPLE_WINDOW, Ok(1.68)));
        assert!((calibration.eye_height().unwrap() - 1.68).abs() < 0.005);

        let span = player().grip_span;
        calibration.measure(t0, Ok(span));
        calibration.tick(start, Ok(span - 0.2));
        calibration.tick(start + Duration::from_millis(100), Ok(span));
        for i in 2..8 {
            calibration.tick(start + Duration::from_millis(100 * i), Ok(span - 0.1));
        }
        assert!(calibration.tick(start + SAMPLE_WINDOW, Ok(span - 0.1)));
        let body = calibration.result().unwrap().unwrap();
        let expected = arm_span_from_grip_span(span, height_from_eye_height(1.68));
        assert!((body.arm_span - expected).abs() < 0.005);
    }

    #[test]
    fn back_retry_and_cancel() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        assert!(!calibration.back());
        assert!(!calibration.retry());
        assert!(!calibration.cancel());

        calibration.start();
        assert!(!calibration.start());
        capture(&mut calibration, t0, player().eye_height);
        assert!(calibration.back());
        assert_eq!(calibration.step(), Step::Stand);
        assert!(calibration.eye_height().is_none());

        capture(&mut calibration, t0, player().eye_height);
        capture(&mut calibration, t0, player().grip_span);
        assert_eq!(calibration.step(), Step::Review);
        // Back from the review keeps the height and measures the span again.
        assert!(calibration.back());
        assert_eq!(calibration.step(), Step::Span);
        assert!(calibration.eye_height().is_some());
        assert!(calibration.result().is_none());
        capture(&mut calibration, t0, player().grip_span);
        assert_eq!(calibration.step(), Step::Review);
        calibration.save_failed("disk full".into());
        assert_eq!(
            calibration.notice(),
            Some(&Notice::SaveFailed("disk full".into()))
        );
        assert!(calibration.retry());
        assert_eq!(calibration.step(), Step::Stand);
        assert!(calibration.result().is_none());
        assert!(calibration.notice().is_none());

        // Back from the standing step leaves the flow.
        assert!(calibration.back());
        assert_eq!(calibration.step(), Step::Summary);
        assert!(!calibration.cancel());

        calibration.start();
        assert!(calibration.cancel());
        assert_eq!(calibration.step(), Step::Summary);
        assert!(calibration.result().is_none());
    }
}
