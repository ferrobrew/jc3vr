//! Body calibration: the dimensions the body file holds, derived from the two things the headset can
//! measure, and the guided flow that measures them.
//!
//! The headset measures the standing eye height (the head above the STAGE floor, see
//! [`crate::vr::stage`]) and, in a T-pose, the distance between the two controllers' grip poses.
//! Every other dimension is derived from those with generic adult proportions ([`derive`]): the
//! segment ratios of Drillis and Contini's anthropometric tables as tabulated in Winter's
//! *Biomechanics and Motor Control of Human Movement*. The height, shoulder, and hand ratios are
//! the ones [`BodyDimensions::GENERIC_ADULT`] follows. The arm segments are not: the tables'
//! per-height upper arm and forearm lengths, with the shoulders and hands, add up to more than an
//! adult's arm span, so the derivation fits the two segments to the measured span instead and
//! takes only their proportion to each other from the tables. The generic adult keeps the tables'
//! longer segments.
//!
//! [`Calibration`] is the flow's state: which step it is on, what it has measured, and the capture
//! in progress. It takes the time and the live measurement as arguments, so it runs without a
//! headset in tests; the pause menu's `VR: BODY` page ([`crate::controllers::body::menu`]) drives
//! it.

use std::time::{Duration, Instant};

use crate::controllers::{
    body::{BodyDimensions, InvalidDimension},
    frame::Hand,
};

/// Standing eye height as a fraction of standing height.
pub const EYE_HEIGHT_RATIO: f32 = 0.933;
/// Standing shoulder-joint height as a fraction of standing height.
pub const SHOULDER_HEIGHT_RATIO: f32 = 0.818;
/// The distance between the shoulder joints as a fraction of standing height.
pub const SHOULDER_WIDTH_RATIO: f32 = 0.223;
/// Hand length (wrist to the tip of the middle finger) as a fraction of standing height.
pub const HAND_RATIO: f32 = 0.108;
/// The upper arm's share of the shoulder-to-wrist length; the forearm takes the rest.
pub const UPPER_ARM_SHARE: f32 = 0.186 / (0.186 + 0.146);
/// How far along the hand, from the wrist, the fingertip lies beyond the grip pose, as a fraction
/// of the hand's length. The grip pose is the centre of the closed hand, about three tenths of the
/// hand's length from the wrist.
pub const GRIP_TO_FINGERTIP_SHARE: f32 = 0.7;

/// The standing eye heights a calibration accepts, metres. Outside this, the player is most likely
/// seated or the runtime's floor is wrong.
pub const EYE_HEIGHT_RANGE: (f32, f32) = (1.0, 2.3);
/// The arm spans a calibration accepts, as a fraction of standing height. An adult's span is
/// within a few percent of their height; outside this range the arms were most likely not spread.
pub const ARM_SPAN_RATIO_RANGE: (f32, f32) = (0.85, 1.15);

/// How long the flow waits after MEASURE is selected, so that the player can take the pose.
pub const COUNTDOWN: Duration = Duration::from_secs(3);
/// How long the flow samples once the countdown ends.
pub const SAMPLE_WINDOW: Duration = Duration::from_secs(1);
/// The fewest samples a capture needs to count.
pub const MIN_SAMPLES: u32 = 5;

/// What the headset measured.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Measurements {
    /// The standing eye height above the floor, metres.
    pub eye_height: f32,
    /// The distance between the two controllers' grip poses in a T-pose, metres.
    pub grip_span: f32,
}

/// Why measurements cannot describe a body.
#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationError {
    /// The eye height is outside [`EYE_HEIGHT_RANGE`], or not a number.
    EyeHeight(f32),
    /// The arm span is outside [`ARM_SPAN_RATIO_RANGE`] of the height, or not a number.
    ArmSpan { arm_span: f32, height: f32 },
    /// A derived dimension failed validation. The ranges above rule this out; it is kept so that a
    /// change to the ratios cannot save an impossible body.
    Invalid(InvalidDimension),
}

impl std::fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "body calibration: ")?;
        match self {
            CalibrationError::EyeHeight(eye) => write!(
                f,
                "an eye height of {eye:.2} m is outside {:.1} to {:.1} m",
                EYE_HEIGHT_RANGE.0, EYE_HEIGHT_RANGE.1
            ),
            CalibrationError::ArmSpan { arm_span, height } => write!(
                f,
                "an arm span of {arm_span:.2} m does not fit a height of {height:.2} m"
            ),
            CalibrationError::Invalid(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CalibrationError {}

/// The standing height of a player whose standing eyes are `eye_height` above the floor.
pub fn height_from_eye_height(eye_height: f32) -> f32 {
    eye_height / EYE_HEIGHT_RATIO
}

/// The fingertip-to-fingertip arm span of a player of `height` whose grip poses are `grip_span`
/// apart in a T-pose: the grip span plus, on each side, the part of the hand beyond the grip.
pub fn arm_span_from_grip_span(grip_span: f32, height: f32) -> f32 {
    grip_span + 2.0 * GRIP_TO_FINGERTIP_SHARE * HAND_RATIO * height
}

/// Derive the body file's dimensions from the measurements, rounded to the millimetre. The height
/// comes from the eye height and the arm span from the grip span; the shoulders and the hand scale
/// with the height, and the arm segments split what the arm span leaves once the shoulders and the
/// hands are taken out of it.
pub fn derive(measurements: Measurements) -> Result<BodyDimensions, CalibrationError> {
    let Measurements {
        eye_height,
        grip_span,
    } = measurements;
    if !(EYE_HEIGHT_RANGE.0..=EYE_HEIGHT_RANGE.1).contains(&eye_height) {
        return Err(CalibrationError::EyeHeight(eye_height));
    }
    let height = height_from_eye_height(eye_height);
    let arm_span = arm_span_from_grip_span(grip_span, height);
    let ratio = arm_span / height;
    if !(ARM_SPAN_RATIO_RANGE.0..=ARM_SPAN_RATIO_RANGE.1).contains(&ratio) {
        return Err(CalibrationError::ArmSpan { arm_span, height });
    }
    let shoulder_width = SHOULDER_WIDTH_RATIO * height;
    let hand = HAND_RATIO * height;
    let shoulder_to_wrist = (arm_span - shoulder_width) * 0.5 - hand;
    let body = BodyDimensions {
        height: millimetres(height),
        eye_height: millimetres(eye_height),
        shoulder_height: millimetres(SHOULDER_HEIGHT_RATIO * height),
        shoulder_width: millimetres(shoulder_width),
        arm_span: millimetres(arm_span),
        upper_arm: millimetres(UPPER_ARM_SHARE * shoulder_to_wrist),
        forearm: millimetres((1.0 - UPPER_ARM_SHARE) * shoulder_to_wrist),
        hand: millimetres(hand),
    };
    body.validate().map_err(CalibrationError::Invalid)?;
    Ok(body)
}

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
    /// This hand's controller position is not tracked.
    ControllerNotTracked(Hand),
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
    /// The measurement became unavailable while sampling.
    Lost(Unavailable),
    /// The sampling window ended with too few samples.
    TooFewSamples,
    /// The measurement cannot describe a body.
    Rejected(CalibrationError),
    /// Saving the body file failed.
    SaveFailed(String),
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
        *self = Self {
            step: Step::Stand,
            ..Self::new()
        };
        true
    }

    /// Start a capture on a measuring step, given the live measurement at `now`. Refuses, with a
    /// notice, when the measurement is unavailable. Returns whether a capture started.
    pub fn measure(&mut self, now: Instant, live: Result<f32, Unavailable>) -> bool {
        if !matches!(self.step, Step::Stand | Step::Span) || self.capture != Capture::Idle {
            return false;
        }
        if let Err(unavailable) = live {
            self.notice = Some(Notice::Refused(unavailable));
            return false;
        }
        self.notice = None;
        self.capture = Capture::Countdown {
            ends: now + COUNTDOWN,
        };
        true
    }

    /// Advance the capture to `now` with the live measurement. The countdown tolerates an
    /// unavailable measurement (the player is moving into the pose); the sampling window does not.
    /// A finished capture moves to the next step when the value is plausible. Returns whether the
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
                    Err(unavailable) => {
                        self.capture = Capture::Idle;
                        self.notice = Some(Notice::Lost(unavailable));
                        return false;
                    }
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

    /// Go back from the T-pose to the standing step. Returns whether the step changed.
    pub fn back(&mut self) -> bool {
        if self.step != Step::Span {
            return false;
        }
        self.step = Step::Stand;
        self.eye_height = None;
        self.capture = Capture::Idle;
        self.notice = None;
        true
    }

    /// Measure again from the standing step, from the review. Returns whether the step changed.
    pub fn retry(&mut self) -> bool {
        if self.step != Step::Review {
            return false;
        }
        self.step = Step::Summary;
        self.start()
    }

    /// Abandon the calibration and return to the summary. Returns whether the step changed.
    pub fn cancel(&mut self) -> bool {
        let changed = self.step != Step::Summary;
        *self = Self::new();
        changed
    }

    /// Record that the result was saved: the flow returns to the summary.
    pub fn saved(&mut self) {
        *self = Self::new();
    }

    /// Record that saving the result failed; the flow stays on the review.
    pub fn save_failed(&mut self, message: String) {
        self.notice = Some(Notice::SaveFailed(message));
    }

    /// Take a finished sampling window's value and move on if it is plausible.
    fn finish(&mut self, samples: Samples) -> bool {
        if samples.count < MIN_SAMPLES {
            self.notice = Some(Notice::TooFewSamples);
            return false;
        }
        match self.step {
            Step::Stand => {
                // Standing still: the mean rides out the head's sway.
                let eye_height = samples.mean();
                if !(EYE_HEIGHT_RANGE.0..=EYE_HEIGHT_RANGE.1).contains(&eye_height) {
                    self.notice = Some(Notice::Rejected(CalibrationError::EyeHeight(eye_height)));
                    return false;
                }
                self.eye_height = Some(eye_height);
                self.step = Step::Span;
                true
            }
            Step::Span => {
                // A T-pose is at its widest when the arms are fully out.
                self.grip_span = Some(samples.max);
                match self.result() {
                    Some(Ok(_)) => {
                        self.step = Step::Review;
                        true
                    }
                    Some(Err(error)) => {
                        self.grip_span = None;
                        self.notice = Some(Notice::Rejected(error));
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

fn millimetres(metres: f32) -> f32 {
    (metres * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-3;

    /// A player of about 1.80 m with a span equal to their height.
    fn player() -> Measurements {
        let height = 1.80;
        Measurements {
            eye_height: height * EYE_HEIGHT_RATIO,
            grip_span: height - 2.0 * GRIP_TO_FINGERTIP_SHARE * HAND_RATIO * height,
        }
    }

    #[test]
    fn derives_a_valid_body_from_the_measurements() {
        let body = derive(player()).unwrap();
        body.validate().unwrap();
        assert!((body.height - 1.80).abs() < EPS);
        assert!((body.arm_span - 1.80).abs() < EPS);
        assert!((body.shoulder_height - 1.80 * SHOULDER_HEIGHT_RATIO).abs() < EPS);
        assert!(body.shoulder_height < body.eye_height && body.eye_height < body.height);
    }

    #[test]
    fn the_arm_adds_up_to_the_span() {
        let body = derive(player()).unwrap();
        let reach = body.upper_arm + body.forearm + body.hand;
        assert!((body.shoulder_width + 2.0 * reach - body.arm_span).abs() < 4.0 * EPS);
        assert!(body.upper_arm > body.forearm);
    }

    #[test]
    fn the_generic_adult_follows_the_same_ratios() {
        let adult = BodyDimensions::GENERIC_ADULT;
        let height = height_from_eye_height(adult.eye_height);
        assert!((height - adult.height).abs() < 0.01);
        assert!((SHOULDER_HEIGHT_RATIO * height - adult.shoulder_height).abs() < 0.02);
        assert!((SHOULDER_WIDTH_RATIO * height - adult.shoulder_width).abs() < 0.01);
        assert!((HAND_RATIO * height - adult.hand).abs() < 0.01);
    }

    #[test]
    fn the_generic_adults_measurements_derive_arms_that_fit_its_span() {
        let adult = BodyDimensions::GENERIC_ADULT;
        let height = height_from_eye_height(adult.eye_height);
        let body = derive(Measurements {
            eye_height: adult.eye_height,
            grip_span: adult.arm_span - 2.0 * GRIP_TO_FINGERTIP_SHARE * HAND_RATIO * height,
        })
        .unwrap();
        assert!((body.arm_span - adult.arm_span).abs() < 2.0 * EPS);
        // The derived segments add up to the span, as the tables' own do not.
        let across =
            |b: &BodyDimensions| b.shoulder_width + 2.0 * (b.upper_arm + b.forearm + b.hand);
        assert!((across(&body) - body.arm_span).abs() < 4.0 * EPS);
        assert!(across(&adult) > adult.arm_span + 0.1);
        // They keep the tables' proportion to each other, and are therefore shorter.
        let proportion = |b: &BodyDimensions| b.upper_arm / b.forearm;
        assert!((proportion(&body) - proportion(&adult)).abs() < 0.02);
        assert!(body.upper_arm < adult.upper_arm && body.forearm < adult.forearm);
        // The rest match the generic adult.
        assert!((body.height - adult.height).abs() < 0.01);
        assert!((body.shoulder_width - adult.shoulder_width).abs() < 0.01);
        assert!((body.hand - adult.hand).abs() < 0.01);
    }

    #[test]
    fn a_longer_span_lengthens_only_the_arms() {
        let base = derive(player()).unwrap();
        let wide = derive(Measurements {
            grip_span: player().grip_span + 0.1,
            ..player()
        })
        .unwrap();
        assert_eq!(wide.height, base.height);
        assert_eq!(wide.shoulder_width, base.shoulder_width);
        assert_eq!(wide.hand, base.hand);
        assert!(wide.upper_arm > base.upper_arm && wide.forearm > base.forearm);
        assert!((wide.arm_span - base.arm_span - 0.1).abs() < 2.0 * EPS);
    }

    #[test]
    fn values_are_rounded_to_the_millimetre() {
        let body = derive(player()).unwrap();
        for value in [body.height, body.upper_arm, body.forearm, body.hand] {
            assert!(((value * 1000.0).round() - value * 1000.0).abs() < 1e-2);
        }
    }

    #[test]
    fn a_seated_eye_height_is_rejected() {
        let err = derive(Measurements {
            eye_height: 0.9,
            ..player()
        })
        .unwrap_err();
        assert!(matches!(err, CalibrationError::EyeHeight(_)));
        let err = derive(Measurements {
            eye_height: f32::NAN,
            ..player()
        })
        .unwrap_err();
        assert!(matches!(err, CalibrationError::EyeHeight(_)));
    }

    #[test]
    fn arms_not_spread_are_rejected() {
        let err = derive(Measurements {
            grip_span: 0.5,
            ..player()
        })
        .unwrap_err();
        assert!(matches!(err, CalibrationError::ArmSpan { .. }));
        let err = derive(Measurements {
            grip_span: 3.0,
            ..player()
        })
        .unwrap_err();
        assert!(matches!(err, CalibrationError::ArmSpan { .. }));
    }

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
        assert_eq!(calibration, Calibration::new());
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
    fn losing_tracking_while_sampling_ends_the_capture() {
        let t0 = Instant::now();
        let mut calibration = Calibration::new();
        calibration.start();
        calibration.measure(t0, Ok(1.6));
        calibration.tick(t0 + COUNTDOWN, Ok(1.6));
        let lost = Unavailable::ControllerNotTracked(Hand::Left);
        assert!(!calibration.tick(t0 + COUNTDOWN, Err(lost)));
        assert_eq!(calibration.notice(), Some(&Notice::Lost(lost)));
        assert_eq!(calibration.step(), Step::Stand);
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
        assert_eq!(calibration.notice(), Some(&Notice::TooFewSamples));
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
            Some(Notice::Rejected(CalibrationError::EyeHeight(_)))
        ));

        assert!(capture(&mut calibration, t0, player().eye_height));
        assert!(!capture(&mut calibration, t0, 0.4));
        assert_eq!(calibration.step(), Step::Span);
        assert!(matches!(
            calibration.notice(),
            Some(Notice::Rejected(CalibrationError::ArmSpan { .. }))
        ));
        assert!(calibration.result().is_none());
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
        calibration.save_failed("disk full".into());
        assert_eq!(
            calibration.notice(),
            Some(&Notice::SaveFailed("disk full".into()))
        );
        assert!(calibration.retry());
        assert_eq!(calibration.step(), Step::Stand);
        assert!(calibration.result().is_none());
        assert!(calibration.notice().is_none());

        assert!(calibration.cancel());
        assert_eq!(calibration, Calibration::new());
    }
}
