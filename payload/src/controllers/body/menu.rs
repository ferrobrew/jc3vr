//! The `VR: BODY` page: the saved body dimensions, the body-scaling switches, and the guided
//! calibration that measures the player in the headset ([`Calibration`]) and saves the result to
//! the body file.
//!
//! The calibration runs on the pause menu's frames: MEASURE starts a countdown so the player can
//! take the pose (a T-pose leaves no hand free to point at the menu), the page's per-frame hook
//! feeds the live measurement to the capture, and a finished capture moves the flow on and has the
//! page rebuilt.

use std::time::Instant;

use glam::Vec3;
use parking_lot::Mutex;
use smol_str::{SmolStr, format_smolstr};

use crate::{
    controllers::{
        body::{
            self, BodyDimensions, BodySource,
            calibration::{
                Calibration, CalibrationError, CaptureStatus, Notice, Step, Unavailable,
                arm_span_from_grip_span, height_from_eye_height,
            },
        },
        frame::{self, Hand, TrackedPose},
        menu::{config_toggle, save_if_changed},
    },
    pause_menu::model::page::{Button, ButtonOutcome, Page, PageDef, Readout},
    vr::{self, stage::StageInfo},
};

/// The `VR: BODY` page.
pub(crate) const PAGE: PageDef = PageDef {
    entry: "VR: BODY",
    build,
    on_close: Some(on_close),
    on_frame: Some(on_frame),
};

fn build() -> Page {
    let step = CALIBRATION.lock().step();
    let page = Page::new("VR: BODY");
    match step {
        Step::Summary => summary(page),
        Step::Stand => stand(page),
        Step::Span => span(page),
        Step::Review => review(page),
    }
}

fn summary(page: Page) -> Page {
    page.header("YOUR BODY")
        .item(Readout {
            label: "SOURCE".into(),
            read: || source_label(&body::player_body().source).into(),
        })
        .item(Readout {
            label: "HEIGHT".into(),
            read: || metres(body::player().height),
        })
        .item(Readout {
            label: "EYE HEIGHT".into(),
            read: || metres(body::player().eye_height),
        })
        .item(Readout {
            label: "ARM SPAN".into(),
            read: || metres(body::player().arm_span),
        })
        .item(Button {
            label: "CALIBRATE".into(),
            caption: "START".into(),
            action: || rebuild_if(CALIBRATION.lock().start()),
        })
        .item(Button {
            label: "RELOAD BODY FILE".into(),
            caption: "RELOAD".into(),
            action: || {
                body::reload();
                ButtonOutcome::Stay
            },
        })
        .text("CALIBRATION MEASURES YOUR HEIGHT AND")
        .text("ARM SPAN WITH THE HEADSET.")
        .header("SCALING")
        .item(config_toggle!("MATCH CHARACTER HEIGHT", |c| c
            .controllers
            .body
            .remap_head))
        .item(config_toggle!("MATCH CHARACTER REACH", |c| c
            .controllers
            .body
            .remap_hands))
}

fn stand(page: Page) -> Page {
    page.header("STEP 1 OF 3: HEIGHT")
        .text("STAND UP STRAIGHT AND LOOK AHEAD.")
        .text("SELECT MEASURE, THEN HOLD STILL")
        .text("FOR A FEW SECONDS.")
        .item(Readout {
            label: "EYE HEIGHT".into(),
            read: || live_text(live_eye_height()),
        })
        .item(Readout {
            label: "HEIGHT".into(),
            read: || live_text(live_eye_height().map(height_from_eye_height)),
        })
        .item(Readout {
            label: "STATUS".into(),
            read: || status(live_eye_height()),
        })
        .item(Button {
            label: "MEASURE HEIGHT".into(),
            caption: "MEASURE".into(),
            action: || measure(live_eye_height()),
        })
        .item(cancel_button())
}

fn span(page: Page) -> Page {
    page.header("STEP 2 OF 3: ARM SPAN")
        .text("HOLD THE CONTROLLERS AND SPREAD YOUR")
        .text("ARMS OUT TO THE SIDES AT SHOULDER")
        .text("HEIGHT. SELECT MEASURE, THEN TAKE")
        .text("THE POSE AND HOLD IT.")
        .item(Readout {
            label: "CONTROLLER SPAN".into(),
            read: || live_text(live_grip_span()),
        })
        .item(Readout {
            label: "ARM SPAN".into(),
            read: || {
                let eye_height = CALIBRATION.lock().eye_height();
                live_text(live_grip_span().map(|grip| {
                    let height = height_from_eye_height(eye_height.unwrap_or(f32::NAN));
                    arm_span_from_grip_span(grip, height)
                }))
            },
        })
        .item(Readout {
            label: "STATUS".into(),
            read: || status(live_grip_span()),
        })
        .item(Button {
            label: "MEASURE ARM SPAN".into(),
            caption: "MEASURE".into(),
            action: || measure(live_grip_span()),
        })
        .item(Button {
            label: "MEASURE HEIGHT AGAIN".into(),
            caption: "BACK".into(),
            action: || rebuild_if(CALIBRATION.lock().back()),
        })
        .item(cancel_button())
}

fn review(page: Page) -> Page {
    page.header("STEP 3 OF 3: REVIEW")
        .text("CHECK THE MEASUREMENTS, THEN SAVE.")
        .text("THE OTHER LENGTHS ARE ESTIMATED FROM")
        .text("YOUR HEIGHT AND ARM SPAN.")
        .item(Readout {
            label: "HEIGHT".into(),
            read: || result_text(|b| b.height),
        })
        .item(Readout {
            label: "EYE HEIGHT".into(),
            read: || result_text(|b| b.eye_height),
        })
        .item(Readout {
            label: "ARM SPAN".into(),
            read: || result_text(|b| b.arm_span),
        })
        .item(Readout {
            label: "SHOULDER HEIGHT".into(),
            read: || result_text(|b| b.shoulder_height),
        })
        .item(Readout {
            label: "SHOULDER WIDTH".into(),
            read: || result_text(|b| b.shoulder_width),
        })
        .item(Readout {
            label: "UPPER ARM".into(),
            read: || result_text(|b| b.upper_arm),
        })
        .item(Readout {
            label: "FOREARM".into(),
            read: || result_text(|b| b.forearm),
        })
        .item(Readout {
            label: "HAND".into(),
            read: || result_text(|b| b.hand),
        })
        .item(Readout {
            label: "STATUS".into(),
            read: || {
                let calibration = CALIBRATION.lock();
                notice_text(calibration.notice()).unwrap_or("READY").into()
            },
        })
        .item(Button {
            label: "SAVE TO BODY FILE".into(),
            caption: "SAVE".into(),
            action: save,
        })
        .item(Button {
            label: "START AGAIN".into(),
            caption: "RETRY".into(),
            action: || rebuild_if(CALIBRATION.lock().retry()),
        })
        .item(cancel_button())
}

fn cancel_button() -> Button {
    Button {
        label: "CANCEL CALIBRATION".into(),
        caption: "CANCEL".into(),
        action: || rebuild_if(CALIBRATION.lock().cancel()),
    }
}

/// A derived dimension for a review readout.
fn result_text(field: fn(&BodyDimensions) -> f32) -> SmolStr {
    match CALIBRATION.lock().result() {
        Some(Ok(body)) => metres(field(&body)),
        Some(Err(_)) | None => "---".into(),
    }
}

/// Start a capture with the live measurement. The status readout shows the countdown, or why the
/// capture was refused.
fn measure(live: Result<f32, Unavailable>) -> ButtonOutcome {
    CALIBRATION.lock().measure(Instant::now(), live);
    ButtonOutcome::Stay
}

/// Save the reviewed dimensions to the body file.
fn save() -> ButtonOutcome {
    let result = CALIBRATION.lock().result();
    let Some(Ok(dimensions)) = result else {
        return ButtonOutcome::Stay;
    };
    match body::save(dimensions) {
        Ok(_) => {
            CALIBRATION.lock().saved();
            ButtonOutcome::Rebuild
        }
        Err(error) => {
            tracing::error!(target: "controllers", "{error}");
            CALIBRATION.lock().save_failed(error.to_string());
            ButtonOutcome::Stay
        }
    }
}

/// Feed the live measurement to a running capture; rebuild when it moved the flow on.
fn on_frame() -> ButtonOutcome {
    let step = CALIBRATION.lock().step();
    let live = match step {
        Step::Stand => live_eye_height(),
        Step::Span => live_grip_span(),
        Step::Summary | Step::Review => return ButtonOutcome::Stay,
    };
    rebuild_if(CALIBRATION.lock().tick(Instant::now(), live))
}

/// Leaving the page abandons a calibration in progress and saves the scaling switches.
fn on_close() {
    CALIBRATION.lock().cancel();
    save_if_changed();
}

/// The calibration flow's state, shared by the page's rows.
static CALIBRATION: Mutex<Calibration> = Mutex::new(Calibration::new());

fn rebuild_if(changed: bool) -> ButtonOutcome {
    if changed {
        ButtonOutcome::Rebuild
    } else {
        ButtonOutcome::Stay
    }
}

/// The head's height above the floor now.
fn live_eye_height() -> Result<f32, Unavailable> {
    eye_height_from(vr::render_params(0).is_some(), vr::stage::stage())
}

/// The distance between the controllers' grips now.
fn live_grip_span() -> Result<f32, Unavailable> {
    let grips = frame::with_latest(|f| Hand::BOTH.map(|hand| f.hand(hand).grip));
    grip_span_from(vr::render_params(0).is_some(), grips)
}

fn eye_height_from(rendering: bool, stage: StageInfo) -> Result<f32, Unavailable> {
    if !rendering {
        return Err(Unavailable::NoHeadset);
    }
    if !stage.available {
        return Err(Unavailable::NoFloor);
    }
    stage.head_height.ok_or(Unavailable::HeadNotTracked)
}

/// The grip span from both hands' grips, indexed by [`Hand::index`]. A grip counts only while its
/// position is measured, not inferred.
fn grip_span_from(rendering: bool, grips: Option<[TrackedPose; 2]>) -> Result<f32, Unavailable> {
    if !rendering {
        return Err(Unavailable::NoHeadset);
    }
    let grips = grips.ok_or(Unavailable::NoControllers)?;
    let position = |hand: Hand| -> Result<Vec3, Unavailable> {
        let grip = grips[hand.index()];
        if grip.position_valid && grip.position_tracked {
            Ok(grip.pose.position)
        } else {
            Err(Unavailable::ControllerNotTracked(hand))
        }
    };
    Ok(position(Hand::Left)?.distance(position(Hand::Right)?))
}

/// A live measurement for a readout.
fn live_text(live: Result<f32, Unavailable>) -> SmolStr {
    match live {
        Ok(value) if value.is_finite() => metres(value),
        Ok(_) => "---".into(),
        Err(unavailable) => unavailable_text(unavailable).into(),
    }
}

/// The status readout of a measuring step: the capture's progress, else the last problem, else
/// whether the measurement is available.
fn status(live: Result<f32, Unavailable>) -> SmolStr {
    let calibration = CALIBRATION.lock();
    status_text(
        calibration.capture_status(Instant::now()),
        calibration.notice(),
        live,
    )
}

fn status_text(
    capture: CaptureStatus,
    notice: Option<&Notice>,
    live: Result<f32, Unavailable>,
) -> SmolStr {
    match capture {
        CaptureStatus::Countdown(seconds) => return format_smolstr!("GET READY: {seconds}"),
        CaptureStatus::Sampling => return "HOLD STILL".into(),
        CaptureStatus::Idle => {}
    }
    if let Some(text) = notice_text(notice) {
        return text.into();
    }
    match live {
        Ok(_) => "READY".into(),
        Err(unavailable) => unavailable_text(unavailable).into(),
    }
}

fn notice_text(notice: Option<&Notice>) -> Option<&'static str> {
    Some(match notice? {
        Notice::Refused(unavailable) => unavailable_text(*unavailable),
        Notice::Lost(_) => "TRACKING LOST, TRY AGAIN",
        Notice::TooFewSamples => "NOT ENOUGH SAMPLES, TRY AGAIN",
        Notice::Rejected(CalibrationError::EyeHeight(_)) => "EYE HEIGHT OUT OF RANGE",
        Notice::Rejected(CalibrationError::ArmSpan { .. }) => "SPREAD YOUR ARMS FULLY",
        Notice::Rejected(CalibrationError::Invalid(_)) => "INVALID MEASUREMENT",
        Notice::SaveFailed(_) => "SAVE FAILED, SEE LOG",
    })
}

fn unavailable_text(unavailable: Unavailable) -> &'static str {
    match unavailable {
        Unavailable::NoHeadset => "NO HEADSET",
        Unavailable::NoFloor => "NO FLOOR HEIGHT",
        Unavailable::HeadNotTracked => "HEADSET NOT TRACKED",
        Unavailable::NoControllers => "NO CONTROLLERS",
        Unavailable::ControllerNotTracked(Hand::Left) => "LEFT CONTROLLER NOT TRACKED",
        Unavailable::ControllerNotTracked(Hand::Right) => "RIGHT CONTROLLER NOT TRACKED",
    }
}

fn source_label(source: &BodySource) -> &'static str {
    match source {
        BodySource::File { .. } => "BODY FILE",
        BodySource::Defaults { .. } => "DEFAULTS, NO FILE",
        BodySource::Invalid { .. } => "DEFAULTS, FILE INVALID",
    }
}

fn metres(value: f32) -> SmolStr {
    format_smolstr!("{value:.2} M")
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::{
        controllers::{frame::Pose, menu::tests::exercise_page},
        pause_menu::model::{page::Item, rows::encode_page},
    };

    fn tracked(position: Vec3) -> TrackedPose {
        TrackedPose {
            pose: Pose {
                position,
                ..Pose::IDENTITY
            },
            position_valid: true,
            orientation_valid: true,
            position_tracked: true,
            orientation_tracked: true,
            ..TrackedPose::default()
        }
    }

    #[test]
    fn the_eye_height_needs_a_headset_and_a_floor() {
        let stage = StageInfo {
            available: true,
            bounds: Some(Vec2::new(2.0, 2.0)),
            head_height: Some(1.68),
        };
        assert_eq!(eye_height_from(true, stage), Ok(1.68));
        assert_eq!(eye_height_from(false, stage), Err(Unavailable::NoHeadset));
        let no_stage = StageInfo::default();
        assert_eq!(eye_height_from(true, no_stage), Err(Unavailable::NoFloor));
        let unlocated = StageInfo {
            head_height: None,
            ..stage
        };
        assert_eq!(
            eye_height_from(true, unlocated),
            Err(Unavailable::HeadNotTracked)
        );
    }

    #[test]
    fn the_grip_span_needs_both_controllers_tracked() {
        let left = tracked(Vec3::new(-0.8, 1.4, 0.0));
        let right = tracked(Vec3::new(0.7, 1.4, 0.0));
        let span = grip_span_from(true, Some([left, right])).unwrap();
        assert!((span - 1.5).abs() < 1e-5);
        assert_eq!(
            grip_span_from(false, Some([left, right])),
            Err(Unavailable::NoHeadset)
        );
        assert_eq!(grip_span_from(true, None), Err(Unavailable::NoControllers));
        let inferred = TrackedPose {
            position_tracked: false,
            ..right
        };
        assert_eq!(
            grip_span_from(true, Some([left, inferred])),
            Err(Unavailable::ControllerNotTracked(Hand::Right))
        );
        assert_eq!(
            grip_span_from(true, Some([TrackedPose::default(), right])),
            Err(Unavailable::ControllerNotTracked(Hand::Left))
        );
    }

    #[test]
    fn the_status_shows_progress_then_problems_then_availability() {
        let refused = Notice::Refused(Unavailable::NoFloor);
        assert_eq!(
            status_text(CaptureStatus::Countdown(2), Some(&refused), Ok(1.0)),
            "GET READY: 2"
        );
        assert_eq!(
            status_text(CaptureStatus::Sampling, None, Err(Unavailable::NoHeadset)),
            "HOLD STILL"
        );
        assert_eq!(
            status_text(CaptureStatus::Idle, Some(&refused), Ok(1.0)),
            "NO FLOOR HEIGHT"
        );
        assert_eq!(
            status_text(CaptureStatus::Idle, None, Err(Unavailable::NoControllers)),
            "NO CONTROLLERS"
        );
        assert_eq!(status_text(CaptureStatus::Idle, None, Ok(1.0)), "READY");
    }

    #[test]
    fn live_values_read_in_metres() {
        assert_eq!(live_text(Ok(1.684)), "1.68 M");
        assert_eq!(live_text(Ok(f32::NAN)), "---");
        assert_eq!(
            live_text(Err(Unavailable::ControllerNotTracked(Hand::Left))),
            "LEFT CONTROLLER NOT TRACKED"
        );
    }

    #[test]
    fn every_step_builds_and_focuses_its_action() {
        let pages = [summary, stand, span, review];
        let first_labels = [
            "CALIBRATE",
            "MEASURE HEIGHT",
            "MEASURE ARM SPAN",
            "SAVE TO BODY FILE",
        ];
        for (build_step, label) in pages.into_iter().zip(first_labels) {
            let page = build_step(Page::new("VR: BODY"));
            encode_page(0, &page).unwrap();
            let Item::Button(first) = &page.items[page.first_button().unwrap()] else {
                unreachable!();
            };
            assert_eq!(first.label, label);
        }
    }

    #[test]
    fn the_scaling_switches_edit_valid_settings() {
        exercise_page(&summary(Page::new("VR: BODY")));
    }
}
