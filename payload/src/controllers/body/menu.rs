//! The `VR: BODY` page: the saved body dimensions, the body-scaling switches, and the guided
//! calibration that measures the player in the headset ([`Calibration`]) and saves the result to
//! the body file.
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

use std::time::{Duration, Instant};

use glam::Vec3;
use parking_lot::Mutex;
use smol_str::{SmolStr, format_smolstr};

use crate::{
    controllers::{
        body::{
            self, BodyDimensions, BodySource,
            calibration::{
                Calibration, CalibrationError, CaptureStatus, Notice, Stance, Step, Unavailable,
                height_from_eye_height, span_out_of_proportion,
            },
        },
        frame::{self, Hand, TrackedPose},
        menu::{config_toggle, read_config, save_if_changed},
        roomscale::{
            self,
            posture::{DetectedPosture, EffectivePosture},
        },
    },
    pause_menu::model::{
        page::{Button, ButtonOutcome, Choice, Page, PageDef, Readout, Slider},
        slider::{SliderFormat, SliderRange},
    },
    vr::{self, stage::StageInfo},
};

/// The `VR: BODY` page.
pub(crate) const PAGE: PageDef = PageDef {
    tab: "BODY",
    build,
    on_close: Some(on_close),
    on_frame: Some(on_frame),
};

fn build() -> Page {
    let (step, stance) = {
        let calibration = CALIBRATION.lock();
        (calibration.step(), calibration.stance())
    };
    let page = Page::new("VR: BODY");
    match (step, stance) {
        (Step::Summary, _) => summary(page),
        (Step::EyeHeight, Stance::Standing) => stand(page),
        (Step::EyeHeight, Stance::Seated) => sit(page),
        (Step::Span, _) => span(page),
        (Step::Review, Stance::Standing) => review_standing(page),
        (Step::Review, Stance::Seated) => review_seated(page),
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
        .item(Readout {
            label: "SEATED EYE HEIGHT".into(),
            read: || optional_metres(body::player().seated_eye_height),
        })
        .header("CALIBRATION")
        .item(Choice {
            label: "STANCE".into(),
            options: &STANCE_OPTIONS,
            get: || stance_index(chosen_stance()),
            set: |index| *STANCE.lock() = Stance::ALL.get(index).copied(),
            default: stance_index(posture_stance()),
        })
        .item(Button {
            label: "CALIBRATE IN HEADSET".into(),
            caption: "START".into(),
            action: start,
        })
        .text("STANDING: HEIGHT AND ARM SPAN.")
        .text("SEATED: SEATED EYES AND ARM SPAN.")
        .item(Button {
            label: "RELOAD BODY FILE".into(),
            caption: "RELOAD".into(),
            action: || {
                body::reload();
                ButtonOutcome::Stay
            },
        })
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
        .text("STAND STRAIGHT AND LOOK AHEAD.")
        .text("SELECT MEASURE, THEN HOLD STILL.")
        .item(Button {
            label: "MEASURE HEIGHT".into(),
            caption: "MEASURE".into(),
            action: || measure(live_eye_height()),
        })
        .item(Readout {
            label: "STATUS".into(),
            read: || status(live_eye_height()),
        })
        .item(Readout {
            label: "EYE HEIGHT, LIVE".into(),
            read: || live_text(live_eye_height()),
        })
        .item(Readout {
            label: "HEIGHT, ESTIMATED".into(),
            read: || live_text(live_eye_height().map(height_from_eye_height)),
        })
        .item(cancel_button())
}

fn sit(page: Page) -> Page {
    page.header("STEP 1 OF 3: SEATED EYES")
        .text("SIT UPRIGHT AND LOOK AHEAD.")
        .text("SELECT MEASURE, THEN HOLD STILL.")
        .item(Button {
            label: "MEASURE SEATED EYES".into(),
            caption: "MEASURE".into(),
            action: || measure(live_eye_height()),
        })
        .item(Readout {
            label: "STATUS".into(),
            read: || status(live_eye_height()),
        })
        .item(Readout {
            label: "EYE HEIGHT, LIVE".into(),
            read: || live_text(live_eye_height()),
        })
        .text("YOUR HEIGHT COMES FROM THE ARM SPAN.")
        .item(cancel_button())
}

fn span(page: Page) -> Page {
    let step_one = match CALIBRATION.lock().stance() {
        Stance::Standing => Readout {
            label: "HEIGHT, FROM STEP 1".into(),
            read: || optional_metres(CALIBRATION.lock().height_before_span()),
        },
        Stance::Seated => Readout {
            label: "SEATED EYES, FROM STEP 1".into(),
            read: || optional_metres(CALIBRATION.lock().eye_height()),
        },
    };
    let page = page
        .header("STEP 2 OF 3: ARM SPAN")
        .text("SELECT MEASURE, THEN T-POSE:")
        .text("ARMS OUT AT SHOULDER HEIGHT.")
        .text("HOLD IT UNTIL THE PAGE MOVES ON.")
        .item(Button {
            label: "MEASURE ARM SPAN".into(),
            caption: "MEASURE".into(),
            action: || measure(live_grip_span()),
        })
        .item(Readout {
            label: "STATUS".into(),
            read: || status(live_grip_span()),
        })
        .item(Readout {
            label: "CONTROLLER GAP, LIVE".into(),
            read: || live_text(live_grip_span()),
        })
        .item(Readout {
            label: "ARM SPAN, ESTIMATED".into(),
            read: || live_estimate(|c, grip| c.arm_span_for(grip)),
        });
    let page = match CALIBRATION.lock().stance() {
        Stance::Standing => page,
        Stance::Seated => page.item(Readout {
            label: "HEIGHT, ESTIMATED".into(),
            read: || live_estimate(|c, grip| c.height_for_span(grip)),
        }),
    };
    page.item(step_one)
        .item(Button {
            label: "MEASURE STEP 1 AGAIN".into(),
            caption: "BACK".into(),
            action: back,
        })
        .item(cancel_button())
}

fn review_standing(page: Page) -> Page {
    let page = review_top(page)
        .header("MEASURED")
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
        });
    let page = estimated_segments(page.header("ESTIMATED"));
    let page = if CALIBRATION.lock().kept_seated_eye_height().is_some() {
        page.header("KEPT FROM THE BODY FILE").item(Readout {
            label: "SEATED EYE HEIGHT".into(),
            read: || optional_metres(CALIBRATION.lock().kept_seated_eye_height()),
        })
    } else {
        page
    };
    review_bottom(page)
}

fn review_seated(page: Page) -> Page {
    let estimate = CALIBRATION.lock().estimated_height();
    let page = review_top(page)
        .header("MEASURED")
        .item(Readout {
            label: "SEATED EYE HEIGHT".into(),
            read: || optional_metres(CALIBRATION.lock().eye_height()),
        })
        .item(Readout {
            label: "ARM SPAN".into(),
            read: || result_text(|b| b.arm_span),
        })
        .header("ESTIMATED")
        .text("HEIGHT IS FROM THE ARM SPAN.")
        .text("SET IT HERE IF IT IS OFF.")
        .item(Slider {
            label: "HEIGHT".into(),
            range: HEIGHT_RANGE,
            format: SliderFormat::new(2, " M"),
            get: || CALIBRATION.lock().height().unwrap_or(f32::NAN),
            set: |height| {
                CALIBRATION.lock().set_height(height);
            },
            default: estimate.map_or(HEIGHT_RANGE.min, |height| {
                height.clamp(HEIGHT_RANGE.min, HEIGHT_RANGE.max)
            }),
        })
        .item(Readout {
            label: "EYE HEIGHT".into(),
            read: || result_text(|b| b.eye_height),
        });
    review_bottom(estimated_segments(page))
}

/// The review's title, instruction, `SAVE`, and `STATUS`.
fn review_top(page: Page) -> Page {
    page.header("STEP 3 OF 3: REVIEW")
        .text("CHECK THE VALUES, THEN SAVE.")
        .item(Button {
            label: "SAVE TO BODY FILE".into(),
            caption: "SAVE".into(),
            action: save,
        })
        .item(Readout {
            label: "STATUS".into(),
            read: review_status,
        })
}

/// The review's readouts of the dimensions scaled from the height and fitted to the span.
fn estimated_segments(page: Page) -> Page {
    page.item(Readout {
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
}

/// The review's way back: measure the span again, start over in the same stance, or cancel.
fn review_bottom(page: Page) -> Page {
    page.header("MEASURE AGAIN")
        .item(Button {
            label: "ARM SPAN ONLY".into(),
            caption: "BACK".into(),
            action: back,
        })
        .item(Button {
            label: "BOTH STEPS".into(),
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

fn back() -> ButtonOutcome {
    rebuild_if(CALIBRATION.lock().back())
}

/// Start the calibration in the chosen stance, keeping the body file's seated eye height for a
/// standing one.
fn start() -> ButtonOutcome {
    let stance = chosen_stance();
    let kept = body::player().seated_eye_height;
    rebuild_if(CALIBRATION.lock().start(stance, kept))
}

/// The `STANCE` choice's options, in [`Stance::ALL`]'s order.
const STANCE_OPTIONS: [&str; 2] = ["STANDING", "SEATED"];

/// The range of the seated review's height slider, metres, a centimetre a notch: the heights whose
/// standing eye heights a standing calibration accepts.
const HEIGHT_RANGE: SliderRange = SliderRange::new(1.2, 2.4, 120);

/// The stance the player chose on the summary since the page opened, `None` for the posture's.
static STANCE: Mutex<Option<Stance>> = Mutex::new(None);

fn chosen_stance() -> Stance {
    STANCE.lock().unwrap_or_else(posture_stance)
}

/// The stance the posture setting implies, with the automatic posture's current detection.
fn posture_stance() -> Stance {
    let detected = roomscale::frame_shared()
        .filter(|f| f.fresh(Instant::now()))
        .map(|f| match f.posture {
            EffectivePosture::Seated => DetectedPosture::Seated,
            EffectivePosture::Standing | EffectivePosture::Roomscale => DetectedPosture::Standing,
        });
    Stance::for_posture(read_config(|c| c.controllers.roomscale.posture), detected)
}

fn stance_index(stance: Stance) -> usize {
    Stance::ALL
        .iter()
        .position(|s| *s == stance)
        .unwrap_or_default()
}

/// A live estimate from the controllers' gap and what the flow has measured so far.
fn live_estimate(estimate: fn(&Calibration, f32) -> Option<f32>) -> SmolStr {
    let grip = live_grip_span();
    let calibration = CALIBRATION.lock();
    live_text(grip.map(|grip| estimate(&calibration, grip).unwrap_or(f32::NAN)))
}

/// A derived dimension for a review readout.
fn result_text(field: fn(&BodyDimensions) -> f32) -> SmolStr {
    match CALIBRATION.lock().result() {
        Some(Ok(body)) => metres(field(&body)),
        Some(Err(_)) | None => "---".into(),
    }
}

/// The review's status: the last problem, else why the result cannot be saved, else whether it is
/// ready to save.
fn review_status() -> SmolStr {
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

/// Start a capture with the live measurement. The status readout shows the countdown, or why the
/// capture was refused.
fn measure(live: Result<f32, Unavailable>) -> ButtonOutcome {
    let mut calibration = CALIBRATION.lock();
    calibration.measure(Instant::now(), live);
    log_notice(&calibration);
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
            let mut calibration = CALIBRATION.lock();
            calibration.save_failed(error.to_string());
            log_notice(&calibration);
            ButtonOutcome::Stay
        }
    }
}

/// Feed the live measurement to a running capture; rebuild when it moved the flow on.
fn on_frame() -> ButtonOutcome {
    let step = CALIBRATION.lock().step();
    let live = match step {
        Step::EyeHeight => live_eye_height(),
        Step::Span => live_grip_span(),
        Step::Summary | Step::Review => return ButtonOutcome::Stay,
    };
    let mut calibration = CALIBRATION.lock();
    let changed = calibration.tick(Instant::now(), live);
    log_notice(&calibration);
    rebuild_if(changed)
}

/// Leaving the page abandons a calibration in progress, returns the stance choice to the
/// posture's, and saves the scaling switches.
fn on_close() {
    CALIBRATION.lock().cancel();
    *STANCE.lock() = None;
    save_if_changed();
}

/// The calibration flow's state, shared by the page's rows.
static CALIBRATION: Mutex<Calibration> = Mutex::new(Calibration::new());

/// Log the flow's notice when it is a new one, at most once per [`NOTICE_LOG_INTERVAL`].
fn log_notice(calibration: &Calibration) {
    let Some(notice) = calibration.notice() else {
        return;
    };
    let suppressed = NOTICE_LOG
        .lock()
        .take(calibration.notice_count(), Instant::now());
    match suppressed {
        Some(0) => tracing::warn!(target: "controllers", "{notice}"),
        Some(suppressed) => {
            tracing::warn!(target: "controllers", "{notice} ({suppressed} earlier not logged)")
        }
        None => {}
    }
}

/// The shortest time between two logged notices.
const NOTICE_LOG_INTERVAL: Duration = Duration::from_secs(1);

static NOTICE_LOG: Mutex<NoticeLog> = Mutex::new(NoticeLog::new());

/// Which notices have been logged, for [`log_notice`]'s rate limit.
#[derive(Debug)]
struct NoticeLog {
    /// The notice count last seen.
    seen: u32,
    /// When a notice was last logged.
    logged_at: Option<Instant>,
    /// New notices not logged since then.
    suppressed: u32,
}

impl NoticeLog {
    const fn new() -> Self {
        Self {
            seen: 0,
            logged_at: None,
            suppressed: 0,
        }
    }

    /// Given the flow's notice count at `now`, whether to log its notice: `Some` with the number
    /// of notices skipped since the last one logged, or `None` when there is no new notice or the
    /// last was logged too recently.
    fn take(&mut self, count: u32, now: Instant) -> Option<u32> {
        let new = count.wrapping_sub(self.seen);
        self.seen = count;
        if new == 0 {
            return None;
        }
        let recent = self
            .logged_at
            .is_some_and(|at| now.saturating_duration_since(at) < NOTICE_LOG_INTERVAL);
        if recent {
            self.suppressed = self.suppressed.saturating_add(new);
            return None;
        }
        self.logged_at = Some(now);
        Some(std::mem::take(&mut self.suppressed).saturating_add(new - 1))
    }
}

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

/// The grip span from both hands' grips, indexed by [`Hand::index`]. A grip counts while its
/// position is valid, whether measured or inferred: arms spread in a T-pose are outside the view
/// of a headset's own cameras, where inside-out tracking reports the controllers' positions as
/// inferred.
fn grip_span_from(rendering: bool, grips: Option<[TrackedPose; 2]>) -> Result<f32, Unavailable> {
    if !rendering {
        return Err(Unavailable::NoHeadset);
    }
    let grips = grips.ok_or(Unavailable::NoControllers)?;
    let position = |hand: Hand| -> Result<Vec3, Unavailable> {
        let grip = grips[hand.index()];
        if grip.position_valid {
            Ok(grip.pose.position)
        } else {
            Err(Unavailable::ControllerLost(hand))
        }
    };
    Ok(position(Hand::Left)?.distance(position(Hand::Right)?))
}

/// A live measurement for a readout.
fn live_text(live: Result<f32, Unavailable>) -> SmolStr {
    match live {
        Ok(value) => metres(value),
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
fn notice_text(notice: Option<&Notice>) -> Option<SmolStr> {
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

fn rejection_text(error: &CalibrationError) -> SmolStr {
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

fn unavailable_text(unavailable: Unavailable) -> &'static str {
    match unavailable {
        Unavailable::NoHeadset => "NO HEADSET",
        Unavailable::NoFloor => "NO FLOOR HEIGHT",
        Unavailable::HeadNotTracked => "HEADSET NOT TRACKED",
        Unavailable::NoControllers => "NO CONTROLLERS",
        Unavailable::ControllerLost(Hand::Left) => "LEFT CONTROLLER LOST",
        Unavailable::ControllerLost(Hand::Right) => "RIGHT CONTROLLER LOST",
    }
}

fn source_label(source: &BodySource) -> &'static str {
    match source {
        BodySource::File { .. } => "BODY FILE",
        BodySource::Defaults { .. } => "DEFAULT, NO FILE",
        BodySource::Invalid { .. } => "DEFAULT, FILE INVALID",
    }
}

/// A length that may be missing, as [`metres`] reads it.
fn optional_metres(value: Option<f32>) -> SmolStr {
    value.map_or("---".into(), metres)
}

/// A length in metres to the centimetre, or `---` when it is not a number.
fn metres(value: f32) -> SmolStr {
    if value.is_finite() {
        format_smolstr!("{value:.2} M")
    } else {
        "---".into()
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::{
        controllers::{
            body::{
                InvalidDimension,
                calibration::{check_eye_height, check_seated_eye_height},
            },
            config::Posture,
            frame::Pose,
            menu::{edit_config, tests::exercise_page},
        },
        pause_menu::model::{page::Item, rows::encode_page},
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
            "SAVE TO BODY FILE",
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
