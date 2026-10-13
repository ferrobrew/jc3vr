//! The `VR: BODY` page: the saved body dimensions, the body-scaling switches, the procedural body,
//! the body turning to face the player's torso, and the guided calibration that measures the
//! player in the headset ([`Calibration`](crate::controllers::body::calibration::Calibration))
//! and saves the result with the rest of the settings.
//!
//! The calibration runs standing or seated ([`Stance`]), as the summary's `STANCE` choice says; it
//! defaults from the posture setting each time the page opens. A seated calibration measures the
//! seated eye height instead of the height, and the review offers a slider for the height it
//! estimates from the arm span.
//!
//! The calibration runs on the pause menu's frames: MEASURE starts a countdown so the player can
//! take the pose (a T-pose leaves no hand free to point at the menu), the page's per-frame hook
//! feeds the live measurement to the capture, and a finished capture moves the flow on and has the
//! page rebuilt. Every refusal shows in the step's `STATUS` readout and is logged, rate-limited.
//!
//! - [`measure`]: the live measurements the calibration steps read;
//! - [`state`]: the running
//!   [`Calibration`](crate::controllers::body::calibration::Calibration), the stance choice, and
//!   the lifecycle handlers;
//! - [`text`]: the row text the flow and its readouts show;
//! - [`pages`]: the page's rows for each step of the flow.

mod measure;
mod pages;
mod state;
mod text;

use crate::{
    controllers::body::calibration::{Stance, Step},
    pause_menu::model::page::{Page, PageDef},
};

/// The `VR: BODY` page.
pub(crate) const PAGE: PageDef = PageDef {
    tab: "BODY",
    build,
    on_close: Some(state::on_close),
    on_frame: Some(state::on_frame),
};

fn build() -> Page {
    let (step, stance) = {
        let calibration = state::CALIBRATION.lock();
        (calibration.step(), calibration.stance())
    };
    let page = Page::new("VR: BODY");
    match (step, stance) {
        (Step::Summary, _) => pages::summary(page),
        (Step::EyeHeight, Stance::Standing) => pages::stand(page),
        (Step::EyeHeight, Stance::Seated) => pages::sit(page),
        (Step::Span, _) => pages::span(page),
        (Step::Review, Stance::Standing) => pages::review_standing(page),
        (Step::Review, Stance::Seated) => pages::review_seated(page),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use glam::{Vec2, Vec3};
    use smol_str::SmolStr;

    use super::*;
    use crate::{
        controllers::{
            body::{
                InvalidDimension,
                calibration::{
                    CalibrationError, CaptureStatus, Notice, Unavailable, check_eye_height,
                    check_seated_eye_height,
                },
                menu::{
                    measure::{eye_height_from, grip_span_from},
                    pages::{
                        STANCE_OPTIONS, review_seated, review_standing, sit, span, stand, summary,
                    },
                    state::{NOTICE_LOG_INTERVAL, NoticeLog},
                    text::{live_text, notice_text, status_text, unavailable_text},
                },
            },
            config::Posture,
            frame::{Hand, Pose, TrackedPose},
            menu::{edit_config, tests::exercise_page},
        },
        pause_menu::model::{page::Item, rows::encode_page},
        vr::stage::StageInfo,
    };

    /// The longest text row that fits one line of the list.
    const TEXT_ROW_MAX: usize = 37;
    /// The longest label that fits beside a row's value.
    const LABEL_MAX: usize = 26;
    /// The longest value a readout shows.
    const VALUE_MAX: usize = 24;

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
            ..StageInfo::default()
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
    fn the_grip_span_needs_both_controllers_located() {
        let left = tracked(Vec3::new(-0.8, 1.4, 0.0));
        let right = tracked(Vec3::new(0.7, 1.4, 0.0));
        let span = grip_span_from(true, Some([left, right])).unwrap();
        assert!((span - 1.5).abs() < 1e-5);
        assert_eq!(
            grip_span_from(false, Some([left, right])),
            Err(Unavailable::NoHeadset)
        );
        assert_eq!(grip_span_from(true, None), Err(Unavailable::NoControllers));
        // An inferred position counts: spread arms are outside an inside-out headset's view.
        let inferred = TrackedPose {
            position_tracked: false,
            ..right
        };
        assert_eq!(grip_span_from(true, Some([left, inferred])), Ok(span));
        let invalid = TrackedPose {
            position_valid: false,
            ..right
        };
        assert_eq!(
            grip_span_from(true, Some([left, invalid])),
            Err(Unavailable::ControllerLost(Hand::Right))
        );
        assert_eq!(
            grip_span_from(true, Some([TrackedPose::default(), right])),
            Err(Unavailable::ControllerLost(Hand::Left))
        );
    }

    #[test]
    fn the_status_shows_progress_then_problems_then_availability() {
        let refused = Notice::Refused(Unavailable::NoFloor);
        assert_eq!(
            status_text(CaptureStatus::Countdown(2), Some(&refused), Ok(1.0)),
            "GET READY: 2 S"
        );
        assert_eq!(
            status_text(CaptureStatus::Sampling, None, Err(Unavailable::NoHeadset)),
            "MEASURING, HOLD STILL"
        );
        assert_eq!(
            status_text(CaptureStatus::Idle, Some(&refused), Ok(1.0)),
            "NO FLOOR HEIGHT"
        );
        assert_eq!(
            status_text(CaptureStatus::Idle, None, Err(Unavailable::NoControllers)),
            "NO CONTROLLERS"
        );
        assert_eq!(
            status_text(CaptureStatus::Idle, None, Ok(1.0)),
            "READY TO MEASURE"
        );
    }

    #[test]
    fn refusals_name_the_reason_and_the_value() {
        let text = |notice: Notice| notice_text(Some(&notice)).unwrap();
        assert_eq!(
            text(Notice::Rejected(check_eye_height(0.95).unwrap_err())),
            "EYES TOO LOW: 0.95 M"
        );
        assert_eq!(
            text(Notice::Rejected(check_eye_height(2.41).unwrap_err())),
            "EYES TOO HIGH: 2.41 M"
        );
        assert_eq!(
            text(Notice::Rejected(CalibrationError::ArmsNotSpread {
                arm_span: 1.1,
                height: 1.8
            })),
            "SPAN TOO SHORT: 1.10 M"
        );
        assert_eq!(
            text(Notice::Rejected(CalibrationError::ArmSpanTooLong(2.61))),
            "SPAN TOO LONG: 2.61 M"
        );
        assert_eq!(
            text(Notice::Rejected(check_seated_eye_height(1.68).unwrap_err())),
            "STANDING? EYES: 1.68 M"
        );
        assert_eq!(
            text(Notice::Rejected(check_seated_eye_height(0.62).unwrap_err())),
            "EYES TOO LOW: 0.62 M"
        );
        assert_eq!(
            text(Notice::TooFewSamples {
                cause: Some(Unavailable::ControllerLost(Hand::Left))
            }),
            "LEFT CONTROLLER LOST"
        );
        assert_eq!(
            text(Notice::TooFewSamples { cause: None }),
            "TOO FEW SAMPLES, RETRY"
        );
        assert_eq!(
            text(Notice::Rejected(CalibrationError::EyeHeightTooLow(
                f32::NAN
            ))),
            "EYES TOO LOW: ---"
        );
    }

    #[test]
    fn every_status_fits_its_row() {
        let unavailable = [
            Unavailable::NoHeadset,
            Unavailable::NoFloor,
            Unavailable::HeadNotTracked,
            Unavailable::NoControllers,
            Unavailable::ControllerLost(Hand::Left),
            Unavailable::ControllerLost(Hand::Right),
        ];
        let mut notices = vec![
            Notice::TooFewSamples { cause: None },
            Notice::SaveFailed("disk full".into()),
            Notice::Rejected(CalibrationError::EyeHeightTooLow(0.95)),
            Notice::Rejected(CalibrationError::EyeHeightTooHigh(12.34)),
            Notice::Rejected(CalibrationError::ArmsNotSpread {
                arm_span: 0.5,
                height: 1.8,
            }),
            Notice::Rejected(CalibrationError::ArmSpanTooLong(12.34)),
            Notice::Rejected(CalibrationError::SeatedEyeHeightTooLow(0.55)),
            Notice::Rejected(CalibrationError::SeatedEyeHeightTooHigh(12.34)),
            Notice::Rejected(CalibrationError::SeatedAboveStanding {
                seated_eye_height: 1.4,
                eye_height: 1.3,
            }),
            Notice::Rejected(CalibrationError::Invalid(InvalidDimension::Misordered {
                lower: "eye_height",
                upper: "height",
            })),
        ];
        for u in unavailable {
            notices.push(Notice::Refused(u));
            notices.push(Notice::TooFewSamples { cause: Some(u) });
        }
        let mut texts: Vec<SmolStr> = notices
            .iter()
            .map(|n| notice_text(Some(n)).unwrap())
            .collect();
        texts.extend(unavailable.map(|u| SmolStr::from(unavailable_text(u))));
        texts.extend([
            status_text(CaptureStatus::Countdown(3), None, Ok(1.0)),
            status_text(CaptureStatus::Sampling, None, Ok(1.0)),
            status_text(CaptureStatus::Idle, None, Ok(1.0)),
            "CHECK: SPAN OVER HEIGHT".into(),
        ]);
        for text in texts {
            assert!(text.len() <= VALUE_MAX, "{text}");
            assert_eq!(text, text.to_uppercase());
        }
    }

    #[test]
    fn live_values_read_in_metres() {
        assert_eq!(live_text(Ok(1.684)), "1.68 M");
        assert_eq!(live_text(Ok(f32::NAN)), "---");
        assert_eq!(
            live_text(Err(Unavailable::ControllerLost(Hand::Left))),
            "LEFT CONTROLLER LOST"
        );
    }

    #[test]
    fn notices_are_logged_once_and_rate_limited() {
        let t0 = Instant::now();
        let mut log = NoticeLog::new();
        assert_eq!(log.take(0, t0), None);
        assert_eq!(log.take(1, t0), Some(0));
        // The same notice is not logged again.
        assert_eq!(log.take(1, t0 + Duration::from_secs(5)), None);
        // New notices within the interval are held back and counted in the next line.
        let soon = t0 + Duration::from_secs(5);
        assert_eq!(log.take(2, soon), Some(0));
        assert_eq!(log.take(3, soon + Duration::from_millis(200)), None);
        assert_eq!(log.take(5, soon + Duration::from_millis(400)), None);
        assert_eq!(log.take(6, soon + NOTICE_LOG_INTERVAL), Some(3));
        assert_eq!(log.take(7, soon + NOTICE_LOG_INTERVAL * 3), Some(0));
    }

    #[test]
    fn every_step_builds_and_focuses_its_action() {
        let pages = [summary, stand, sit, span, review_standing, review_seated];
        let first_labels = [
            "CALIBRATE IN HEADSET",
            "MEASURE HEIGHT",
            "MEASURE SEATED EYES",
            "MEASURE ARM SPAN",
            "SAVE DIMENSIONS",
            "SAVE DIMENSIONS",
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
    fn every_calibration_step_can_go_back_and_cancel() {
        for build_step in [stand, sit, span, review_standing, review_seated] {
            let page = build_step(Page::new("VR: BODY"));
            let captions: Vec<&str> = page
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Button(button) => Some(button.caption.as_str()),
                    _ => None,
                })
                .collect();
            assert!(captions.contains(&"CANCEL"), "{captions:?}");
        }
        // The first step's way back is its cancel; the later steps also step back one.
        for build_step in [span, review_standing, review_seated] {
            let page = build_step(Page::new("VR: BODY"));
            assert!(page.items.iter().any(|item| matches!(
                item,
                Item::Button(button) if button.caption == "BACK"
            )));
        }
    }

    #[test]
    fn every_step_fits_its_rows() {
        for build_step in [summary, stand, sit, span, review_standing, review_seated] {
            let page = build_step(Page::new("VR: BODY"));
            assert!(page.is_header(0));
            for item in &page.items {
                let (text, max) = match item {
                    Item::Header(text) | Item::Text(text) => (text.as_str(), TEXT_ROW_MAX),
                    Item::Readout(readout) => (readout.label.as_str(), LABEL_MAX),
                    Item::Button(button) => (button.label.as_str(), LABEL_MAX),
                    Item::Toggle(toggle) => (toggle.label.as_str(), LABEL_MAX),
                    Item::Choice(_) | Item::Slider(_) => continue,
                };
                assert!(text.len() <= max, "{text}");
            }
        }
    }

    #[test]
    fn the_stance_choice_defaults_from_the_posture() {
        let stance_default = || {
            summary(Page::new("VR: BODY"))
                .items
                .iter()
                .find_map(|item| match item {
                    Item::Choice(choice) if choice.label == "STANCE" => Some(choice.default),
                    _ => None,
                })
                .unwrap()
        };
        edit_config(|c| c.controllers.roomscale.posture = Posture::Seated);
        assert_eq!(STANCE_OPTIONS[stance_default()], "SEATED");
        edit_config(|c| c.controllers.roomscale.posture = Posture::Auto);
        assert_eq!(STANCE_OPTIONS[stance_default()], "STANDING");
    }

    #[test]
    fn the_scaling_switches_edit_valid_settings() {
        exercise_page(&summary(Page::new("VR: BODY")));
    }
}
