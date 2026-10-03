//! The body dimensions derived from what the headset measures, and the bounds a measurement must
//! fall within.

use crate::controllers::body::{BodyDimensions, InvalidDimension};

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

/// An adult's arm span as a fraction of their standing height. The two are within a few percent of
/// each other for most adults, so a seated calibration, which cannot measure the height, estimates
/// it from the span ([`height_from_grip_span`]).
pub const ARM_SPAN_HEIGHT_RATIO: f32 = 1.0;

/// The standing eye heights a calibration accepts, metres. Outside this, the player is most likely
/// seated or the runtime's floor is wrong.
pub const EYE_HEIGHT_RANGE: (f32, f32) = (1.0, 2.3);
/// The seated eye heights a calibration accepts, metres: from a low sofa to a tall stool. Above
/// this, the player is most likely standing.
pub const SEATED_EYE_HEIGHT_RANGE: (f32, f32) = (0.7, 1.5);
/// The arm spans a calibration accepts, metres, whatever the height.
pub const ARM_SPAN_RANGE: (f32, f32) = (1.2, 2.4);
/// The shortest arm span a calibration accepts, as a fraction of the height. An adult's span is
/// within a few percent of their height, so a shorter one means the arms were not spread. There is
/// no upper bound relative to the height: the grip span cannot exceed the arms, so a span well over
/// the height means the height is low (the eye height was measured seated), not the arms long, and
/// the review flags it instead ([`span_out_of_proportion`]).
pub const ARM_SPAN_MIN_RATIO: f32 = 0.85;
/// The arm span, as a fraction of the height, above which the review flags the two as out of
/// proportion.
pub const ARM_SPAN_FLAG_RATIO: f32 = 1.15;

/// What the headset measured from a standing player.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Measurements {
    /// The standing eye height above the floor, metres.
    pub eye_height: f32,
    /// The distance between the two controllers' grip poses in a T-pose, metres.
    pub grip_span: f32,
}

/// What the headset measured from a seated player, and the height the player set, if any.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SeatedMeasurements {
    /// The eye height above the floor while sitting upright, metres.
    pub seated_eye_height: f32,
    /// The distance between the two controllers' grip poses in a T-pose, metres.
    pub grip_span: f32,
    /// The standing height the player set on the review, replacing the estimate from the span.
    pub height: Option<f32>,
}

/// Why measurements cannot describe a body.
#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationError {
    /// The eye height is below [`EYE_HEIGHT_RANGE`], or not a number.
    EyeHeightTooLow(f32),
    /// The eye height is above [`EYE_HEIGHT_RANGE`].
    EyeHeightTooHigh(f32),
    /// The seated eye height is below [`SEATED_EYE_HEIGHT_RANGE`], or not a number.
    SeatedEyeHeightTooLow(f32),
    /// The seated eye height is above [`SEATED_EYE_HEIGHT_RANGE`].
    SeatedEyeHeightTooHigh(f32),
    /// The seated eyes are not below the standing eyes of the height set for the player.
    SeatedAboveStanding {
        seated_eye_height: f32,
        eye_height: f32,
    },
    /// The arm span is below [`ARM_SPAN_RANGE`] or [`ARM_SPAN_MIN_RATIO`] of the height, or not a
    /// number: the arms were not spread.
    ArmsNotSpread { arm_span: f32, height: f32 },
    /// The arm span is above [`ARM_SPAN_RANGE`]: a tracking error, since no arm reaches that far.
    ArmSpanTooLong(f32),
    /// A derived dimension failed validation. The ranges above rule this out; it is kept so that a
    /// change to the ratios cannot save an impossible body.
    Invalid(InvalidDimension),
}

impl std::fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "body calibration: ")?;
        match self {
            CalibrationError::EyeHeightTooLow(eye) => write!(
                f,
                "an eye height of {eye:.2} m is below the {:.1} m minimum",
                EYE_HEIGHT_RANGE.0
            ),
            CalibrationError::EyeHeightTooHigh(eye) => write!(
                f,
                "an eye height of {eye:.2} m is above the {:.1} m maximum",
                EYE_HEIGHT_RANGE.1
            ),
            CalibrationError::SeatedEyeHeightTooLow(eye) => write!(
                f,
                "a seated eye height of {eye:.2} m is below the {:.1} m minimum",
                SEATED_EYE_HEIGHT_RANGE.0
            ),
            CalibrationError::SeatedEyeHeightTooHigh(eye) => write!(
                f,
                "a seated eye height of {eye:.2} m is above the {:.1} m maximum",
                SEATED_EYE_HEIGHT_RANGE.1
            ),
            CalibrationError::SeatedAboveStanding {
                seated_eye_height,
                eye_height,
            } => write!(
                f,
                "a seated eye height of {seated_eye_height:.2} m is not below the standing eye \
                 height of {eye_height:.2} m"
            ),
            CalibrationError::ArmsNotSpread { arm_span, height } => write!(
                f,
                "an arm span of {arm_span:.2} m is too short for a height of {height:.2} m \
                 (the minimum is {:.2} m)",
                min_arm_span(*height)
            ),
            CalibrationError::ArmSpanTooLong(arm_span) => write!(
                f,
                "an arm span of {arm_span:.2} m is above the {:.1} m maximum",
                ARM_SPAN_RANGE.1
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

/// The standing height of a player whose grip poses are `grip_span` apart in a T-pose, estimated
/// from the arm span: the height at which [`arm_span_from_grip_span`] gives a span of
/// [`ARM_SPAN_HEIGHT_RATIO`] of the height.
pub fn height_from_grip_span(grip_span: f32) -> f32 {
    grip_span / (ARM_SPAN_HEIGHT_RATIO - 2.0 * GRIP_TO_FINGERTIP_SHARE * HAND_RATIO)
}

/// The shortest arm span a calibration accepts for a player of `height`.
pub fn min_arm_span(height: f32) -> f32 {
    (ARM_SPAN_MIN_RATIO * height).max(ARM_SPAN_RANGE.0)
}

/// Whether `body`'s arm span is so far over its height that the height is most likely wrong.
pub fn span_out_of_proportion(body: &BodyDimensions) -> bool {
    body.arm_span > ARM_SPAN_FLAG_RATIO * body.height
}

/// Check a standing eye height against [`EYE_HEIGHT_RANGE`].
pub fn check_eye_height(eye_height: f32) -> Result<(), CalibrationError> {
    if eye_height > EYE_HEIGHT_RANGE.1 {
        return Err(CalibrationError::EyeHeightTooHigh(eye_height));
    }
    if eye_height.is_nan() || eye_height < EYE_HEIGHT_RANGE.0 {
        return Err(CalibrationError::EyeHeightTooLow(eye_height));
    }
    Ok(())
}

/// Check a seated eye height against [`SEATED_EYE_HEIGHT_RANGE`].
pub fn check_seated_eye_height(seated_eye_height: f32) -> Result<(), CalibrationError> {
    if seated_eye_height > SEATED_EYE_HEIGHT_RANGE.1 {
        return Err(CalibrationError::SeatedEyeHeightTooHigh(seated_eye_height));
    }
    if seated_eye_height.is_nan() || seated_eye_height < SEATED_EYE_HEIGHT_RANGE.0 {
        return Err(CalibrationError::SeatedEyeHeightTooLow(seated_eye_height));
    }
    Ok(())
}

/// Derive the body file's dimensions from a standing player's measurements, rounded to the
/// millimetre. The height comes from the eye height and the arm span from the grip span; the
/// shoulders and the hand scale with the height, and the arm segments split what the arm span
/// leaves once the shoulders and the hands are taken out of it. The result has no seated eye
/// height.
pub fn derive(measurements: Measurements) -> Result<BodyDimensions, CalibrationError> {
    let Measurements {
        eye_height,
        grip_span,
    } = measurements;
    check_eye_height(eye_height)?;
    let height = height_from_eye_height(eye_height);
    proportions(height, eye_height, grip_span)
}

/// Derive the body file's dimensions from a seated player's measurements, rounded to the
/// millimetre. The height is the one the player set, else the estimate from the span
/// ([`height_from_grip_span`]); the standing eye height follows from it, and everything else is
/// derived as [`derive`] does. The seated eye height is kept as measured.
pub fn derive_seated(measurements: SeatedMeasurements) -> Result<BodyDimensions, CalibrationError> {
    let SeatedMeasurements {
        seated_eye_height,
        grip_span,
        height,
    } = measurements;
    check_seated_eye_height(seated_eye_height)?;
    let height = height.unwrap_or_else(|| height_from_grip_span(grip_span));
    let eye_height = EYE_HEIGHT_RATIO * height;
    check_eye_height(eye_height)?;
    if seated_eye_height >= eye_height {
        return Err(CalibrationError::SeatedAboveStanding {
            seated_eye_height,
            eye_height,
        });
    }
    let mut body = proportions(height, eye_height, grip_span)?;
    body.seated_eye_height = Some(millimetres(seated_eye_height));
    body.validate().map_err(CalibrationError::Invalid)?;
    Ok(body)
}

/// A standing calibration's result with the seated eye height from the body file it replaces,
/// when that is still below the new standing eye height: a standing calibration does not measure
/// it, and saving would otherwise drop it.
pub fn keeping_seated_eye_height(body: BodyDimensions, kept: Option<f32>) -> BodyDimensions {
    let seated_eye_height =
        kept.filter(|seated| seated.is_finite() && *seated > 0.0 && *seated < body.eye_height);
    BodyDimensions {
        seated_eye_height,
        ..body
    }
}

/// The dimensions of a player of `height`, with standing eyes at `eye_height`, whose grip poses are
/// `grip_span` apart in a T-pose.
fn proportions(
    height: f32,
    eye_height: f32,
    grip_span: f32,
) -> Result<BodyDimensions, CalibrationError> {
    let arm_span = arm_span_from_grip_span(grip_span, height);
    if arm_span > ARM_SPAN_RANGE.1 {
        return Err(CalibrationError::ArmSpanTooLong(arm_span));
    }
    if arm_span.is_nan() || arm_span < min_arm_span(height) {
        return Err(CalibrationError::ArmsNotSpread { arm_span, height });
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
        seated_eye_height: None,
    };
    body.validate().map_err(CalibrationError::Invalid)?;
    Ok(body)
}

fn millimetres(metres: f32) -> f32 {
    (metres * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::calibration::tests::{SEATED_EYE_HEIGHT, player, seated_player};

    const EPS: f32 = 1e-3;

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
        assert!(matches!(err, CalibrationError::EyeHeightTooLow(_)));
        let err = derive(Measurements {
            eye_height: f32::NAN,
            ..player()
        })
        .unwrap_err();
        assert!(matches!(err, CalibrationError::EyeHeightTooLow(_)));
        let err = derive(Measurements {
            eye_height: 2.4,
            ..player()
        })
        .unwrap_err();
        assert!(matches!(err, CalibrationError::EyeHeightTooHigh(_)));
    }

    #[test]
    fn arms_not_spread_are_rejected() {
        let err = derive(Measurements {
            grip_span: 0.5,
            ..player()
        })
        .unwrap_err();
        assert!(matches!(err, CalibrationError::ArmsNotSpread { .. }));
        let err = derive(Measurements {
            grip_span: f32::NAN,
            ..player()
        })
        .unwrap_err();
        assert!(matches!(err, CalibrationError::ArmsNotSpread { .. }));
        let err = derive(Measurements {
            grip_span: 3.0,
            ..player()
        })
        .unwrap_err();
        assert!(matches!(err, CalibrationError::ArmSpanTooLong(_)));
    }

    #[test]
    fn a_full_span_is_accepted_over_a_low_height() {
        // A seated eye height makes the height short; the span is still the arms' own.
        let body = derive(Measurements {
            eye_height: SEATED_EYE_HEIGHT,
            ..player()
        })
        .unwrap();
        body.validate().unwrap();
        assert!(body.arm_span > ARM_SPAN_FLAG_RATIO * body.height);
        assert!(span_out_of_proportion(&body));
        assert!(!span_out_of_proportion(&derive(player()).unwrap()));
    }

    #[test]
    fn the_height_estimate_from_the_span_inverts_the_span_from_the_height() {
        for height in [1.5, 1.8, 2.0] {
            let grip_span = ARM_SPAN_HEIGHT_RATIO * height
                - 2.0 * GRIP_TO_FINGERTIP_SHARE * HAND_RATIO * height;
            let estimate = height_from_grip_span(grip_span);
            assert!((estimate - height).abs() < 1e-4, "{height}");
            assert!(
                (arm_span_from_grip_span(grip_span, estimate) - ARM_SPAN_HEIGHT_RATIO * estimate)
                    .abs()
                    < 1e-4
            );
        }
    }

    #[test]
    fn a_seated_calibration_estimates_the_standing_body_from_the_span() {
        let body = derive_seated(seated_player()).unwrap();
        body.validate().unwrap();
        // The standing player of the same span.
        let standing = derive(player()).unwrap();
        assert_eq!(body.seated_eye_height, Some(SEATED_EYE_HEIGHT));
        assert!((body.height - standing.height).abs() < EPS);
        assert!((body.eye_height - standing.eye_height).abs() < EPS);
        assert!((body.arm_span - standing.arm_span).abs() < EPS);
        assert!(!span_out_of_proportion(&body));
        assert_eq!(standing.seated_eye_height, None);
    }

    #[test]
    fn a_corrected_height_carries_the_derived_values_with_it() {
        let estimate = derive_seated(seated_player()).unwrap();
        let corrected = derive_seated(SeatedMeasurements {
            height: Some(1.70),
            ..seated_player()
        })
        .unwrap();
        assert!((corrected.height - 1.70).abs() < EPS);
        assert!((corrected.eye_height - EYE_HEIGHT_RATIO * 1.70).abs() < EPS);
        assert!((corrected.shoulder_height - SHOULDER_HEIGHT_RATIO * 1.70).abs() < EPS);
        assert!((corrected.shoulder_width - SHOULDER_WIDTH_RATIO * 1.70).abs() < EPS);
        assert!((corrected.hand - HAND_RATIO * 1.70).abs() < EPS);
        // The span is still the grips' own, with the fingertips of the smaller hands.
        let span = arm_span_from_grip_span(seated_player().grip_span, 1.70);
        assert!((corrected.arm_span - span).abs() < EPS);
        assert!(corrected.arm_span < estimate.arm_span);
        // What the arm leaves once the shoulders and the hands are out still adds up to the span.
        let across = corrected.shoulder_width
            + 2.0 * (corrected.upper_arm + corrected.forearm + corrected.hand);
        assert!((across - corrected.arm_span).abs() < 4.0 * EPS);
        assert_eq!(corrected.seated_eye_height, estimate.seated_eye_height);
    }

    #[test]
    fn a_seated_calibration_refuses_implausible_seated_eyes() {
        let with = |seated_eye_height, height| {
            derive_seated(SeatedMeasurements {
                seated_eye_height,
                height,
                ..seated_player()
            })
        };
        assert!(matches!(
            with(0.5, None),
            Err(CalibrationError::SeatedEyeHeightTooLow(_))
        ));
        assert!(matches!(
            with(f32::NAN, None),
            Err(CalibrationError::SeatedEyeHeightTooLow(_))
        ));
        // A standing player's eye height.
        assert!(matches!(
            with(1.68, None),
            Err(CalibrationError::SeatedEyeHeightTooHigh(_))
        ));
        // A height set so short that the seated eyes are over the standing ones.
        assert!(matches!(
            with(1.4, Some(1.45)),
            Err(CalibrationError::SeatedAboveStanding { .. })
        ));
        // A height no standing adult has.
        assert!(matches!(
            with(1.2, Some(1.0)),
            Err(CalibrationError::EyeHeightTooLow(_))
        ));
    }

    #[test]
    fn a_standing_result_keeps_a_seated_eye_height_below_its_eyes() {
        let body = derive(player()).unwrap();
        let kept = keeping_seated_eye_height(body, Some(SEATED_EYE_HEIGHT));
        assert_eq!(kept.seated_eye_height, Some(SEATED_EYE_HEIGHT));
        kept.validate().unwrap();
        let dropped = keeping_seated_eye_height(body, Some(body.eye_height + 0.1));
        assert_eq!(dropped.seated_eye_height, None);
        assert_eq!(
            keeping_seated_eye_height(body, None).seated_eye_height,
            None
        );
        assert_eq!(
            keeping_seated_eye_height(body, Some(f32::NAN)).seated_eye_height,
            None
        );
    }

    #[test]
    fn every_accepted_measurement_derives_a_valid_body() {
        let heights = (0..26)
            .map(|i| EYE_HEIGHT_RANGE.0 + i as f32 * 0.05)
            .chain([EYE_HEIGHT_RANGE.1]);
        for eye_height in heights {
            let height = height_from_eye_height(eye_height);
            for i in 0..=30 {
                let grip_span = 0.8 + i as f32 * 0.06;
                let arm_span = arm_span_from_grip_span(grip_span, height);
                match derive(Measurements {
                    eye_height,
                    grip_span,
                }) {
                    Ok(body) => body.validate().unwrap(),
                    Err(CalibrationError::ArmsNotSpread { .. }) => {
                        assert!(arm_span < min_arm_span(height))
                    }
                    Err(CalibrationError::ArmSpanTooLong(_)) => {
                        assert!(arm_span > ARM_SPAN_RANGE.1)
                    }
                    Err(other) => panic!("{other}"),
                }
            }
        }
    }
}
