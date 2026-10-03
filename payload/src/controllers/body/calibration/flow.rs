//! The calibration flow: its steps, the capture that samples a live measurement, and what went
//! wrong.

use std::time::{Duration, Instant};

use crate::controllers::{
    body::{
        BodyDimensions,
        calibration::{
            CalibrationError, Measurements, SeatedMeasurements, arm_span_from_grip_span,
            check_eye_height, check_seated_eye_height, derive, derive_seated,
            height_from_eye_height, height_from_grip_span, keeping_seated_eye_height,
        },
    },
    config::Posture,
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

/// How the player is calibrated: standing, which measures the height, or seated, which measures
/// the seated eye height and estimates the height from the arm span.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Stance {
    Standing,
    Seated,
}

impl Stance {
    /// Both stances, in the order the page lists them.
    pub const ALL: [Stance; 2] = [Stance::Standing, Stance::Seated];

    /// The stance a calibration defaults to under the posture setting: seated in the seated
    /// posture, standing otherwise (the automatic posture never resolves to seated).
    pub fn for_posture(posture: Posture) -> Stance {
        match posture {
            Posture::Seated => Stance::Seated,
            Posture::Auto | Posture::Standing | Posture::Roomscale => Stance::Standing,
        }
    }
}

/// A step of the calibration flow.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Step {
    /// Not calibrating: the page shows the saved dimensions.
    Summary,
    /// Standing straight or sitting upright, by the stance, to measure the eye height.
    EyeHeight,
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
    stance: Stance,
    /// The seated eye height of the body file being replaced, which a standing calibration keeps.
    kept_seated_eye_height: Option<f32>,
    /// The eye height measured in the stance.
    eye_height: Option<f32>,
    grip_span: Option<f32>,
    /// The standing height a seated player set on the review.
    height: Option<f32>,
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
            stance: Stance::Standing,
            kept_seated_eye_height: None,
            eye_height: None,
            grip_span: None,
            height: None,
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

    /// The stance being calibrated.
    pub fn stance(&self) -> Stance {
        self.stance
    }

    /// The eye height measured in the stance, once the eye-height step is done.
    pub fn eye_height(&self) -> Option<f32> {
        self.eye_height
    }

    /// The seated eye height a standing calibration keeps from the body file it replaces.
    pub fn kept_seated_eye_height(&self) -> Option<f32> {
        match self.stance {
            Stance::Standing => self.kept_seated_eye_height,
            Stance::Seated => None,
        }
    }

    /// The standing height before the arm span is measured: from the standing eye height, and
    /// unknown seated, where it comes from the span.
    pub fn height_before_span(&self) -> Option<f32> {
        match self.stance {
            Stance::Standing => self.eye_height.map(height_from_eye_height),
            Stance::Seated => None,
        }
    }

    /// The standing height a grip span of `grip_span` gives with what is measured so far: the
    /// standing calibration's measured height, or the seated calibration's estimate from the span.
    pub fn height_for_span(&self, grip_span: f32) -> Option<f32> {
        match self.stance {
            Stance::Standing => self.height_before_span(),
            Stance::Seated => Some(height_from_grip_span(grip_span)),
        }
    }

    /// The fingertip arm span a grip span of `grip_span` gives with what is measured so far, for
    /// the arm-span step's live readout.
    pub fn arm_span_for(&self, grip_span: f32) -> Option<f32> {
        Some(arm_span_from_grip_span(
            grip_span,
            self.height_for_span(grip_span)?,
        ))
    }

    /// The seated calibration's standing height estimated from the measured span, once it is
    /// measured. The player's own setting replaces it ([`Calibration::set_height`]).
    pub fn estimated_height(&self) -> Option<f32> {
        match self.stance {
            Stance::Seated => self.grip_span.map(height_from_grip_span),
            Stance::Standing => None,
        }
    }

    /// The standing height on the review: the player's setting, else the estimate (seated), or the
    /// measured one (standing).
    pub fn height(&self) -> Option<f32> {
        match self.stance {
            Stance::Seated => self.height.or(self.estimated_height()),
            Stance::Standing => self.height_before_span(),
        }
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

    /// The dimensions derived from both measurements, once both are taken: the standing body with
    /// the kept seated eye height, or the seated body with its standing height estimated or set.
    pub fn result(&self) -> Option<Result<BodyDimensions, CalibrationError>> {
        let eye_height = self.eye_height?;
        let grip_span = self.grip_span?;
        Some(match self.stance {
            Stance::Standing => derive(Measurements {
                eye_height,
                grip_span,
            })
            .map(|body| keeping_seated_eye_height(body, self.kept_seated_eye_height)),
            Stance::Seated => derive_seated(SeatedMeasurements {
                seated_eye_height: eye_height,
                grip_span,
                height: self.height,
            }),
        })
    }

    /// Begin calibrating in `stance`, from the summary. `kept_seated_eye_height` is the body file's
    /// seated eye height, which a standing calibration keeps. Returns whether the step changed.
    pub fn start(&mut self, stance: Stance, kept_seated_eye_height: Option<f32>) -> bool {
        if self.step != Step::Summary {
            return false;
        }
        self.stance = stance;
        self.kept_seated_eye_height = kept_seated_eye_height;
        self.reset(Step::EyeHeight);
        true
    }

    /// Set the seated calibration's standing height on the review, replacing the estimate from the
    /// span. Returns whether it was taken: only a seated review has a height to set, and only a
    /// finite one.
    pub fn set_height(&mut self, height: f32) -> bool {
        if self.step != Step::Review || self.stance != Stance::Seated || !height.is_finite() {
            return false;
        }
        self.height = Some(height);
        true
    }

    /// Start a capture on a measuring step, given the live measurement at `now`. Refuses, with a
    /// notice, when the measurement is unavailable. Returns whether a capture started.
    pub fn measure(&mut self, now: Instant, live: Result<f32, Unavailable>) -> bool {
        if !matches!(self.step, Step::EyeHeight | Step::Span) || self.capture != Capture::Idle {
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

    /// Go back one step: from the T-pose to the eye-height step, from the review to the T-pose
    /// (keeping the eye height, and dropping a height set on the review, which the new span
    /// re-estimates), or from the eye-height step to the summary. Returns whether the step
    /// changed.
    pub fn back(&mut self) -> bool {
        match self.step {
            Step::Summary => false,
            Step::EyeHeight => self.cancel(),
            Step::Span => {
                self.step = Step::EyeHeight;
                self.eye_height = None;
                self.capture = Capture::Idle;
                self.notice = None;
                true
            }
            Step::Review => {
                self.step = Step::Span;
                self.grip_span = None;
                self.height = None;
                self.notice = None;
                true
            }
        }
    }

    /// Measure again from the eye-height step, in the same stance, from the review. Returns
    /// whether the step changed.
    pub fn retry(&mut self) -> bool {
        if self.step != Step::Review {
            return false;
        }
        self.reset(Step::EyeHeight);
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

    /// Clear the measurements and the notice and go to `step`, keeping the stance, the kept seated
    /// eye height, and the notice count.
    fn reset(&mut self, step: Step) {
        *self = Self {
            step,
            stance: self.stance,
            kept_seated_eye_height: self.kept_seated_eye_height,
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
            Step::EyeHeight => {
                // Holding still: the mean rides out the head's sway.
                let eye_height = samples.mean();
                let checked = match self.stance {
                    Stance::Standing => check_eye_height(eye_height),
                    Stance::Seated => check_seated_eye_height(eye_height),
                };
                if let Err(error) = checked {
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
        EYE_HEIGHT_RATIO, span_out_of_proportion,
        tests::{SEATED_EYE_HEIGHT, player, seated_player},
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
        assert!(calibration.start(Stance::Standing, None));
        assert_eq!(calibration.step(), Step::EyeHeight);

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
        calibration.start(Stance::Standing, None);
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
        calibration.start(Stance::Standing, None);
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
        calibration.start(Stance::Standing, None);
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
        calibration.start(Stance::Standing, None);
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
        calibration.start(Stance::Standing, None);
        calibration.measure(t0, Ok(1.6));
        calibration.tick(t0 + COUNTDOWN, Ok(1.6));
        assert!(!calibration.tick(t0 + COUNTDOWN + SAMPLE_WINDOW, Ok(1.6)));
        assert_eq!(
            calibration.notice(),
            Some(&Notice::TooFewSamples { cause: None })
        );
        assert_eq!(calibration.step(), Step::EyeHeight);
    }

    #[test]
    fn an_implausible_capture_stays_on_its_step() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start(Stance::Standing, None);
        assert!(!capture(&mut calibration, t0, 0.8));
        assert_eq!(calibration.step(), Step::EyeHeight);
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
    fn the_stance_defaults_to_seated_only_in_the_seated_posture() {
        assert_eq!(Stance::for_posture(Posture::Seated), Stance::Seated);
        for posture in [Posture::Auto, Posture::Standing, Posture::Roomscale] {
            assert_eq!(Stance::for_posture(posture), Stance::Standing);
        }
    }

    #[test]
    fn a_seated_calibration_saves_a_standing_height_and_the_seated_eyes() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        assert!(calibration.start(Stance::Seated, None));
        assert_eq!(calibration.stance(), Stance::Seated);
        assert_eq!(calibration.step(), Step::EyeHeight);
        assert!(capture(&mut calibration, t0, SEATED_EYE_HEIGHT));
        assert_eq!(calibration.step(), Step::Span);
        // The height waits for the span.
        assert_eq!(calibration.height_before_span(), None);
        assert_eq!(calibration.height(), None);
        let grip_span = player().grip_span;
        assert!((calibration.height_for_span(grip_span).unwrap() - 1.80).abs() < 1e-4);
        assert!((calibration.arm_span_for(grip_span).unwrap() - 1.80).abs() < 1e-4);

        assert!(capture(&mut calibration, t0, grip_span));
        assert_eq!(calibration.step(), Step::Review);
        assert!(calibration.notice().is_none());
        let body = calibration.result().unwrap().unwrap();
        assert_eq!(body, derive_seated(seated_player()).unwrap());
        // Not the seated eye height as the standing one.
        assert!((body.height - 1.80).abs() < 1e-3);
        assert!(body.eye_height > SEATED_EYE_HEIGHT);
        assert_eq!(body.seated_eye_height, Some(SEATED_EYE_HEIGHT));
        assert!(!span_out_of_proportion(&body));
    }

    #[test]
    fn a_seated_review_takes_a_height_correction_until_the_span_is_measured_again() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start(Stance::Seated, None);
        // No height to set before the review.
        assert!(!calibration.set_height(1.7));
        capture(&mut calibration, t0, SEATED_EYE_HEIGHT);
        capture(&mut calibration, t0, player().grip_span);
        let estimate = calibration.estimated_height().unwrap();
        assert_eq!(calibration.height(), Some(estimate));

        assert!(calibration.set_height(1.7));
        assert!(!calibration.set_height(f32::NAN));
        assert_eq!(calibration.height(), Some(1.7));
        assert_eq!(calibration.estimated_height(), Some(estimate));
        let body = calibration.result().unwrap().unwrap();
        assert!((body.height - 1.7).abs() < 1e-3);
        assert!((body.eye_height - EYE_HEIGHT_RATIO * 1.7).abs() < 1e-3);
        assert_eq!(
            body,
            derive_seated(SeatedMeasurements {
                height: Some(1.7),
                ..seated_player()
            })
            .unwrap()
        );

        // A height that puts the standing eyes under the seated ones is refused on the review.
        calibration.set_height(1.25);
        assert!(matches!(
            calibration.result(),
            Some(Err(CalibrationError::SeatedAboveStanding { .. }))
        ));

        // Back to the span drops the setting; the new span is estimated afresh.
        assert!(calibration.back());
        assert_eq!(calibration.height(), None);
        capture(&mut calibration, t0, player().grip_span);
        assert_eq!(calibration.height(), Some(estimate));
        // Retry keeps the stance.
        assert!(calibration.retry());
        assert_eq!(calibration.stance(), Stance::Seated);
        assert_eq!(calibration.step(), Step::EyeHeight);
    }

    #[test]
    fn a_seated_step_refuses_a_standing_eye_height_and_a_standing_step_a_seated_one() {
        let t0 = Instant::now();
        let mut seated = Calibration::new();
        seated.start(Stance::Seated, None);
        assert!(!capture(&mut seated, t0, player().eye_height));
        assert_eq!(seated.step(), Step::EyeHeight);
        assert!(matches!(
            seated.notice(),
            Some(Notice::Rejected(CalibrationError::SeatedEyeHeightTooHigh(
                _
            )))
        ));

        let mut standing = Calibration::new();
        standing.start(Stance::Standing, None);
        assert!(!capture(&mut standing, t0, 0.9));
        assert!(matches!(
            standing.notice(),
            Some(Notice::Rejected(CalibrationError::EyeHeightTooLow(_)))
        ));
    }

    #[test]
    fn a_standing_calibration_keeps_the_seated_eye_height_and_has_no_height_to_set() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start(Stance::Standing, Some(SEATED_EYE_HEIGHT));
        assert_eq!(
            calibration.kept_seated_eye_height(),
            Some(SEATED_EYE_HEIGHT)
        );
        capture(&mut calibration, t0, player().eye_height);
        capture(&mut calibration, t0, player().grip_span);
        assert!(!calibration.set_height(1.7));
        assert_eq!(calibration.estimated_height(), None);
        let body = calibration.result().unwrap().unwrap();
        assert_eq!(body.seated_eye_height, Some(SEATED_EYE_HEIGHT));
        assert_eq!(
            body,
            keeping_seated_eye_height(derive(player()).unwrap(), Some(SEATED_EYE_HEIGHT))
        );
        // A seated calibration replaces the kept value with its own measurement.
        let mut seated = Calibration::new();
        seated.start(Stance::Seated, Some(1.0));
        assert_eq!(seated.kept_seated_eye_height(), None);
    }

    #[test]
    fn a_standing_calibration_taken_seated_is_still_flagged() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start(Stance::Standing, None);
        assert!(capture(&mut calibration, t0, SEATED_EYE_HEIGHT));
        assert!(capture(&mut calibration, t0, player().grip_span));
        assert_eq!(calibration.step(), Step::Review);
        assert!(span_out_of_proportion(
            &calibration.result().unwrap().unwrap()
        ));
    }

    #[test]
    fn every_notice_is_counted() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start(Stance::Standing, None);
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
        calibration.start(Stance::Standing, None);
        calibration.measure(t0, Err(Unavailable::NoHeadset));
        assert_eq!(calibration.notice_count(), 3);
    }

    #[test]
    fn the_span_takes_the_widest_sample_and_the_height_the_mean() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start(Stance::Standing, None);
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

        calibration.start(Stance::Standing, None);
        assert!(!calibration.start(Stance::Standing, None));
        capture(&mut calibration, t0, player().eye_height);
        assert!(calibration.back());
        assert_eq!(calibration.step(), Step::EyeHeight);
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
        assert_eq!(calibration.step(), Step::EyeHeight);
        assert!(calibration.result().is_none());
        assert!(calibration.notice().is_none());

        // Back from the standing step leaves the flow.
        assert!(calibration.back());
        assert_eq!(calibration.step(), Step::Summary);
        assert!(!calibration.cancel());

        calibration.start(Stance::Standing, None);
        assert!(calibration.cancel());
        assert_eq!(calibration.step(), Step::Summary);
        assert!(calibration.result().is_none());
    }
}
