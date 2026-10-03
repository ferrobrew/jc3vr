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

/// The standing eye heights a calibration accepts, metres. Outside this, the player is most likely
/// seated or the runtime's floor is wrong.
pub const EYE_HEIGHT_RANGE: (f32, f32) = (1.0, 2.3);
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
    /// The eye height is below [`EYE_HEIGHT_RANGE`], or not a number.
    EyeHeightTooLow(f32),
    /// The eye height is above [`EYE_HEIGHT_RANGE`].
    EyeHeightTooHigh(f32),
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

/// Derive the body file's dimensions from the measurements, rounded to the millimetre. The height
/// comes from the eye height and the arm span from the grip span; the shoulders and the hand scale
/// with the height, and the arm segments split what the arm span leaves once the shoulders and the
/// hands are taken out of it.
pub fn derive(measurements: Measurements) -> Result<BodyDimensions, CalibrationError> {
    let Measurements {
        eye_height,
        grip_span,
    } = measurements;
    check_eye_height(eye_height)?;
    let height = height_from_eye_height(eye_height);
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
    use crate::controllers::body::calibration::tests::{SEATED_EYE_HEIGHT, player};

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
