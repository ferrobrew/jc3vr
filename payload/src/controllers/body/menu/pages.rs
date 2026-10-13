//! The `VR: BODY` page's rows for each step of the flow: the summary, the two measuring steps,
//! and the review for each stance.

use crate::{
    controllers::{
        body::{
            self,
            calibration::{Stance, height_from_eye_height},
            menu::{
                measure::{live_eye_height, live_grip_span},
                state::{
                    CALIBRATION, STANCE, chosen_stance, measure, posture_stance, rebuild_if, save,
                    stance_index,
                },
                text::{
                    live_estimate, live_text, metres, optional_metres, result_text, review_status,
                    source_label, status,
                },
            },
            procedural::config::ProceduralMode,
        },
        menu::{config_choice, config_toggle},
    },
    pause_menu::model::{
        page::{Button, ButtonOutcome, Choice, Page, Readout, Slider},
        slider::{SliderFormat, SliderRange},
    },
};

/// The `STANCE` choice's options, in [`Stance::ALL`]'s order.
pub(super) const STANCE_OPTIONS: [&str; 2] = ["STANDING", "SEATED"];

pub(super) const PROCEDURAL_MODES: [ProceduralMode; 3] = [
    ProceduralMode::Off,
    ProceduralMode::PassThrough,
    ProceduralMode::Full,
];
pub(super) const PROCEDURAL_MODE_LABELS: [&str; 3] = ["OFF", "TEST", "FULL"];

/// The range of the seated review's height slider, metres, a centimetre a notch: the heights whose
/// standing eye heights a standing calibration accepts.
pub(super) const HEIGHT_RANGE: SliderRange = SliderRange::new(1.2, 2.4, 120);

pub(super) fn summary(page: Page) -> Page {
    page.header("YOUR BODY")
        .item(Readout {
            label: "SOURCE".into(),
            read: || source_label(body::is_generic()).into(),
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
        .header("SCALING")
        .item(config_toggle!("MATCH CHARACTER HEIGHT", |c| c
            .controllers
            .body
            .remap_head))
        .item(config_toggle!("MATCH CHARACTER REACH", |c| c
            .controllers
            .body
            .remap_hands))
        .header("PROCEDURAL BODY")
        .item(config_choice!(
            "BODY FROM YOUR HEAD",
            &PROCEDURAL_MODES,
            &PROCEDURAL_MODE_LABELS,
            |c| c.controllers.body.procedural.mode
        ))
        .item(config_toggle!("WALKING LEGS", |c| c
            .controllers
            .body
            .procedural
            .gait))
        .item(config_toggle!("STEP UNDER YOUR HEAD", |c| c
            .controllers
            .body
            .procedural
            .step_under_head))
        .item(config_toggle!("SHORT STEPS IN A CROUCH", |c| c
            .controllers
            .body
            .procedural
            .crouch_gait))
        .item(config_toggle!("KNEEL BELOW A SQUAT", |c| c
            .controllers
            .body
            .procedural
            .kneel))
        .item(config_toggle!("SIT ON THE GROUND", |c| c
            .controllers
            .body
            .procedural
            .sit))
        .item(config_toggle!("LEGS IN THE AIR", |c| c
            .controllers
            .body
            .procedural
            .air_legs))
        .item(config_toggle!("LEGS REACH FOR LANDINGS", |c| c
            .controllers
            .body
            .procedural
            .landing_reach))
        .item(config_toggle!("HANDS OUT OF THE BODY", |c| c
            .controllers
            .body
            .procedural
            .keep_out))
        .item(config_toggle!("SHOULDERS FOLLOW THE HANDS", |c| c
            .controllers
            .body
            .procedural
            .clavicles))
        .item(config_toggle!("SPREAD THE LIMB TWIST", |c| c
            .controllers
            .body
            .procedural
            .twist_bones))
        .item(config_toggle!("BODY AT THE FRAME RATE", |c| c
            .controllers
            .hands
            .presentation
            .enabled))
        .item(config_toggle!("TORSO IN VEHICLES", |c| c
            .controllers
            .body
            .procedural
            .seated_torso))
        .item(config_toggle!("ARMS AS WINGS", |c| c
            .controllers
            .body
            .procedural
            .wingsuit_arms))
        .item(config_toggle!("BODY UNDER THE PARACHUTE", |c| c
            .controllers
            .body
            .procedural
            .parachute_body))
        .item(config_toggle!("TORSO AND ARMS SWIMMING", |c| c
            .controllers
            .body
            .procedural
            .swim_torso))
        .item(config_toggle!("TORSO ON VEHICLE ROOFS", |c| c
            .controllers
            .body
            .procedural
            .stunt_torso))
        .text("TEST WRITES THE POSE BACK AS IS.")
        .header("ALIGNMENT")
        .item(config_toggle!("BODY FACES YOUR TORSO", |c| c
            .controllers
            .body
            .rig
            .torso
            .follow))
        .text("ONLY THE TURN INPUT TURNS THE VIEW.")
}

pub(super) fn stand(page: Page) -> Page {
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

pub(super) fn sit(page: Page) -> Page {
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

pub(super) fn span(page: Page) -> Page {
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

pub(super) fn review_standing(page: Page) -> Page {
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
        page.header("KEPT FROM THE SAVED BODY").item(Readout {
            label: "SEATED EYE HEIGHT".into(),
            read: || optional_metres(CALIBRATION.lock().kept_seated_eye_height()),
        })
    } else {
        page
    };
    review_bottom(page)
}

pub(super) fn review_seated(page: Page) -> Page {
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
            label: "SAVE DIMENSIONS".into(),
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

/// Start the calibration in the chosen stance, keeping the saved seated eye height for a
/// standing one.
fn start() -> ButtonOutcome {
    let stance = chosen_stance();
    let kept = body::player().seated_eye_height;
    rebuild_if(CALIBRATION.lock().start(stance, kept))
}
