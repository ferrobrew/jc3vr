//! The row text the calibration flow and its readouts show: measurements in metres, the live and
//! review statuses, and the notice and refusal reasons.

use std::time::Instant;

use smol_str::{SmolStr, format_smolstr};

use crate::controllers::{
    body::{
        BodyDimensions,
        calibration::{
            Calibration, CalibrationError, CaptureStatus, Notice, Unavailable,
            span_out_of_proportion,
        },
        menu::{measure::live_grip_span, state::CALIBRATION},
    },
    frame::Hand,
};

/// A live estimate from the controllers' gap and what the flow has measured so far.
pub(super) fn live_estimate(estimate: fn(&Calibration, f32) -> Option<f32>) -> SmolStr {
    let grip = live_grip_span();
    let calibration = CALIBRATION.lock();
    live_text(grip.map(|grip| estimate(&calibration, grip).unwrap_or(f32::NAN)))
}

/// A derived dimension for a review readout.
pub(super) fn result_text(field: fn(&BodyDimensions) -> f32) -> SmolStr {
    match CALIBRATION.lock().result() {
        Some(Ok(body)) => metres(field(&body)),
        Some(Err(_)) | None => "---".into(),
    }
}

/// The review's status: the last problem, else why the result cannot be saved, else whether it is
/// ready to save.
pub(super) fn review_status() -> SmolStr {
    let calibration = CALIBRATION.lock();
    if let Some(text) = notice_text(calibration.notice()) {
        return text;
    }
    match calibration.result() {
        Some(Ok(body)) if span_out_of_proportion(&body) => "CHECK: SPAN OVER HEIGHT".into(),
        Some(Ok(_)) => "READY TO SAVE".into(),
        Some(Err(error)) => rejection_text(&error),
        None => "---".into(),
    }
}

/// The status readout of a measuring step: the capture's progress, else the last problem, else
/// whether the measurement is available.
pub(super) fn status(live: Result<f32, Unavailable>) -> SmolStr {
    let calibration = CALIBRATION.lock();
    status_text(
        calibration.capture_status(Instant::now()),
        calibration.notice(),
        live,
    )
}

pub(super) fn status_text(
    capture: CaptureStatus,
    notice: Option<&Notice>,
    live: Result<f32, Unavailable>,
) -> SmolStr {
    match capture {
        CaptureStatus::Countdown(seconds) => return format_smolstr!("GET READY: {seconds} S"),
        CaptureStatus::Sampling => return "MEASURING, HOLD STILL".into(),
        CaptureStatus::Idle => {}
    }
    if let Some(text) = notice_text(notice) {
        return text;
    }
    match live {
        Ok(_) => "READY TO MEASURE".into(),
        Err(unavailable) => unavailable_text(unavailable).into(),
    }
}

/// The status text for a notice: what went wrong, with the value that was refused.
pub(super) fn notice_text(notice: Option<&Notice>) -> Option<SmolStr> {
    Some(match notice? {
        Notice::Refused(unavailable) => unavailable_text(*unavailable).into(),
        Notice::TooFewSamples {
            cause: Some(unavailable),
        } => unavailable_text(*unavailable).into(),
        Notice::TooFewSamples { cause: None } => "TOO FEW SAMPLES, RETRY".into(),
        Notice::Rejected(error) => rejection_text(error),
        Notice::SaveFailed(_) => "SAVE FAILED, SEE LOG".into(),
    })
}

pub(super) fn rejection_text(error: &CalibrationError) -> SmolStr {
    match error {
        CalibrationError::EyeHeightTooLow(eye) => format_smolstr!("EYES TOO LOW: {}", metres(*eye)),
        CalibrationError::EyeHeightTooHigh(eye) => {
            format_smolstr!("EYES TOO HIGH: {}", metres(*eye))
        }
        CalibrationError::SeatedEyeHeightTooLow(eye) => {
            format_smolstr!("EYES TOO LOW: {}", metres(*eye))
        }
        CalibrationError::SeatedEyeHeightTooHigh(eye) => {
            format_smolstr!("STANDING? EYES: {}", metres(*eye))
        }
        CalibrationError::SeatedAboveStanding { .. } => "HEIGHT TOO SHORT".into(),
        CalibrationError::ArmsNotSpread { arm_span, .. } => {
            format_smolstr!("SPAN TOO SHORT: {}", metres(*arm_span))
        }
        CalibrationError::ArmSpanTooLong(arm_span) => {
            format_smolstr!("SPAN TOO LONG: {}", metres(*arm_span))
        }
        CalibrationError::Invalid(_) => "INVALID BODY, SEE LOG".into(),
    }
}

pub(super) fn unavailable_text(unavailable: Unavailable) -> &'static str {
    match unavailable {
        Unavailable::NoHeadset => "NO HEADSET",
        Unavailable::NoFloor => "NO FLOOR HEIGHT",
        Unavailable::HeadNotTracked => "HEADSET NOT TRACKED",
        Unavailable::NoControllers => "NO CONTROLLERS",
        Unavailable::ControllerLost(Hand::Left) => "LEFT CONTROLLER LOST",
        Unavailable::ControllerLost(Hand::Right) => "RIGHT CONTROLLER LOST",
    }
}

pub(super) fn source_label(generic: bool) -> &'static str {
    if generic { "GENERIC DEFAULT" } else { "SAVED" }
}

/// A length that may be missing, as [`metres`] reads it.
pub(super) fn optional_metres(value: Option<f32>) -> SmolStr {
    value.map_or("---".into(), metres)
}

/// A length in metres to the centimetre, or `---` when it is not a number.
pub(super) fn metres(value: f32) -> SmolStr {
    if value.is_finite() {
        format_smolstr!("{value:.2} M")
    } else {
        "---".into()
    }
}

/// A live measurement for a readout.
pub(super) fn live_text(live: Result<f32, Unavailable>) -> SmolStr {
    match live {
        Ok(value) => metres(value),
        Err(unavailable) => unavailable_text(unavailable).into(),
    }
}
