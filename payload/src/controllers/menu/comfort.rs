//! The `VR: COMFORT` page: recentering the view, the posture, the body following the head, the
//! fade when the head is in a wall, turning, the stick movement, and the view in vehicles: getting
//! in, and the driver's view at the wheel. The recenter is an action, not a setting. The comfort
//! tuning (the fades' times, the crouch, the lean, the stick's rates, the view on the grapple) is
//! in the debug UI.

use std::time::{Duration, Instant};

use parking_lot::Mutex;
use smol_str::SmolStr;

use crate::{
    controllers::{
        config::{Posture, SeatTransition},
        menu::{
            config_choice, config_slider, config_toggle, edit_config, nearest_index, read_config,
        },
        roomscale::manual_recenter_kind,
    },
    headpose::config::{VrTurnConfig, VrTurnMode},
    pause_menu::model::{
        page::{Button, ButtonOutcome, Choice, Page, PageDef, Readout},
        slider::{SliderFormat, SliderRange},
    },
    vr::{recenter_count, render_params, request_recenter},
};

/// The `VR: COMFORT` page.
pub(crate) const PAGE: PageDef = PageDef {
    tab: "COMFORT",
    build,
    on_close: None,
    on_frame: None,
};

fn build() -> Page {
    Page::new("VR: COMFORT")
        .header("VIEW")
        .item(Button {
            label: "RECENTER VIEW".into(),
            caption: "RECENTER".into(),
            action: recenter_now,
        })
        .item(Readout {
            label: "STATUS".into(),
            read: recenter_status_text,
        })
        .header("POSTURE")
        .item(config_choice!("POSTURE", &POSTURES, &POSTURE_LABELS, |c| c
            .controllers
            .roomscale
            .posture))
        .item(config_toggle!("BODY FOLLOWS YOU", |c| c
            .controllers
            .body
            .rig
            .chase
            .enabled))
        .item(config_toggle!("FADE WHEN HEAD IS IN WALLS", |c| c
            .controllers
            .roomscale
            .head_fade
            .enabled))
        .header("TURNING")
        .item(config_choice!(
            "TURN MODE",
            &TURN_MODES,
            &TURN_MODE_LABELS,
            |c| c.headpose.vr_turn.mode
        ))
        .item(config_slider!(
            "SMOOTH TURN SPEED",
            TURN_SPEED_RANGE,
            SliderFormat::new(2, "X"),
            |c| c.headpose.vr_turn.smooth_scale
        ))
        .item(Choice {
            label: SNAP_TURN_ANGLE.into(),
            options: &SNAP_ANGLE_LABELS,
            get: || {
                nearest_index(
                    &SNAP_ANGLES,
                    read_config(|c| c.headpose.vr_turn.snap_angle_deg),
                )
            },
            set: |index| {
                if let Some(&angle) = SNAP_ANGLES.get(index) {
                    edit_config(|c| c.headpose.vr_turn.snap_angle_deg = angle);
                }
            },
            default: nearest_index(&SNAP_ANGLES, VrTurnConfig::new().snap_angle_deg),
        })
        .header("MOVEMENT")
        .item(config_toggle!("SMOOTH STICK MOVEMENT", |c| c
            .controllers
            .locomotion
            .stick
            .enabled))
        .header("VEHICLES")
        .item(config_choice!(
            "VEHICLE ENTRY",
            &SEAT_TRANSITIONS,
            &SEAT_TRANSITION_LABELS,
            |c| c.controllers.roomscale.seat_transition
        ))
        .item(config_toggle!("FIT VIEW TO THE WHEEL", |c| c
            .controllers
            .vehicles
            .driving_view
            .enabled))
}

/// The recenter button: ask for a recenter of the kind the F7 key does, at the start of the next
/// frame. The menu runs on the UI capture thread in the middle of a frame, where a recenter in
/// place could wait on the frame's own runtime lock.
fn recenter_now() -> ButtonOutcome {
    // The count is read before the request, which the frame thread may take at once.
    let press = RecenterPress {
        at: Instant::now(),
        count: recenter_count(),
    };
    request_recenter(manual_recenter_kind());
    *LAST_PRESS.lock() = Some(press);
    ButtonOutcome::Stay
}

/// The recenter row's status, refreshed every frame.
///
/// The menu updates on a UI worker while the frame thread holds the VR runtime lock and waits for
/// that worker, so the session check reads the draw-safe render-parameter slot rather than the
/// runtime state, as the body page does.
fn recenter_status_text() -> SmolStr {
    recenter_status(
        render_params(0).is_some(),
        *LAST_PRESS.lock(),
        Instant::now(),
        recenter_count(),
    )
    .into()
}

/// A press of the recenter button: when, and the recenter count then.
#[derive(Copy, Clone, Debug)]
struct RecenterPress {
    at: Instant,
    count: u32,
}

/// The last press of the recenter button.
static LAST_PRESS: Mutex<Option<RecenterPress>> = Mutex::new(None);

/// How long the outcome of a press shows.
const OUTCOME_SHOWN: Duration = Duration::from_secs(3);
/// How long a request may wait for its frame before it counts as not done. A frame takes it at
/// its start, so a running headset takes it well within this.
const REQUEST_WAIT: Duration = Duration::from_secs(1);

/// The status for a headset that is `running`, the last press `press`, the time `now`, and the
/// recenter count `count`: the outcome of a recent press, else the prompt.
fn recenter_status(
    running: bool,
    press: Option<RecenterPress>,
    now: Instant,
    count: u32,
) -> &'static str {
    if !running {
        return "NO HEADSET";
    }
    if let Some(press) = press {
        let elapsed = now.saturating_duration_since(press.at);
        let done = count != press.count;
        if done && elapsed < OUTCOME_SHOWN {
            return "RECENTERED";
        }
        if !done && elapsed < REQUEST_WAIT {
            return "RECENTERING";
        }
        // A running headset drops a request only without a head pose, which it logs.
        if !done && elapsed < OUTCOME_SHOWN {
            return "NOT DONE, SEE LOG";
        }
    }
    "LOOK AHEAD, THEN PRESS"
}

const POSTURES: [Posture; 3] = [Posture::Auto, Posture::Seated, Posture::Standing];
const POSTURE_LABELS: [&str; 3] = ["AUTO", "SEATED", "STANDING"];

const SEAT_TRANSITIONS: [SeatTransition; 3] = [
    SeatTransition::Ride,
    SeatTransition::Fade,
    SeatTransition::Instant,
];
const SEAT_TRANSITION_LABELS: [&str; 3] = ["FOLLOW THE ANIMATION", "FADE", "INSTANT"];

/// The snap-turn angle's row.
const SNAP_TURN_ANGLE: &str = "SNAP TURN ANGLE";

const TURN_MODES: [VrTurnMode; 2] = [VrTurnMode::Smooth, VrTurnMode::Snap];
const TURN_MODE_LABELS: [&str; 2] = ["SMOOTH", "SNAP"];

/// The snap-turn steps offered, degrees. A saved angle between them shows as the nearest.
const SNAP_ANGLES: [f32; 5] = [15.0, 30.0, 45.0, 60.0, 90.0];
const SNAP_ANGLE_LABELS: [&str; 5] = [
    "15 DEGREES",
    "30 DEGREES",
    "45 DEGREES",
    "60 DEGREES",
    "90 DEGREES",
];

// Slider notch counts are multiples of ten: the movie's keyboard step is a tenth of the notches,
// truncated to an integer.

/// The smooth-turn scale, in twentieths.
const TURN_SPEED_RANGE: SliderRange = SliderRange::new(0.5, 3.0, 50);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{controllers::menu::tests::exercise_page, pause_menu::model::page::Item};

    #[test]
    fn the_page_edits_valid_settings_and_resets_to_the_defaults() {
        exercise_page(&build());
    }

    #[test]
    fn the_recenter_status_follows_the_request() {
        let start = Instant::now();
        let press = Some(RecenterPress {
            at: start,
            count: 7,
        });
        let at = |ms: u64| start + Duration::from_millis(ms);

        assert_eq!(recenter_status(false, press, at(0), 7), "NO HEADSET");
        assert_eq!(
            recenter_status(true, None, at(0), 7),
            "LOOK AHEAD, THEN PRESS"
        );
        assert_eq!(recenter_status(true, press, at(10), 7), "RECENTERING");
        assert_eq!(recenter_status(true, press, at(10), 8), "RECENTERED");
        assert_eq!(recenter_status(true, press, at(2_900), 8), "RECENTERED");
        assert_eq!(
            recenter_status(true, press, at(3_000), 8),
            "LOOK AHEAD, THEN PRESS"
        );
        assert_eq!(
            recenter_status(true, press, at(1_000), 7),
            "NOT DONE, SEE LOG"
        );
        assert_eq!(
            recenter_status(true, press, at(3_000), 7),
            "LOOK AHEAD, THEN PRESS"
        );
        // The count wraps; any change means the request ran.
        let wrapping = Some(RecenterPress {
            at: start,
            count: u32::MAX,
        });
        assert_eq!(recenter_status(true, wrapping, at(10), 0), "RECENTERED");
    }

    #[test]
    fn the_recenter_texts_fit_the_readout() {
        // A readout's value fits in about 24 characters beside its label.
        for text in [
            "NO HEADSET",
            "LOOK AHEAD, THEN PRESS",
            "RECENTERING",
            "RECENTERED",
            "NOT DONE, SEE LOG",
        ] {
            assert!(text.len() <= 24, "{text}");
            assert_eq!(text, text.to_uppercase());
        }
    }

    #[test]
    fn the_page_opens_on_the_recenter_button() {
        assert_eq!(build().first_button(), Some(1));
    }

    #[test]
    fn the_choice_lists_line_up() {
        assert_eq!(POSTURES.len(), POSTURE_LABELS.len());
        assert_eq!(SEAT_TRANSITIONS.len(), SEAT_TRANSITION_LABELS.len());
        assert_eq!(TURN_MODES.len(), TURN_MODE_LABELS.len());
        assert_eq!(SNAP_ANGLES.len(), SNAP_ANGLE_LABELS.len());
        assert_eq!(
            SNAP_ANGLES[nearest_index(&SNAP_ANGLES, VrTurnConfig::new().snap_angle_deg)],
            VrTurnConfig::new().snap_angle_deg,
            "the default snap angle is offered",
        );
    }

    #[test]
    fn a_snap_angle_between_the_steps_shows_as_the_nearest() {
        let page = build();
        let Some(Item::Choice(snap)) = page
            .items
            .iter()
            .find(|item| matches!(item, Item::Choice(c) if c.label == SNAP_TURN_ANGLE))
        else {
            panic!("no snap angle choice");
        };
        edit_config(|c| c.headpose.vr_turn.snap_angle_deg = 40.0);
        assert_eq!((snap.get)(), 2);
    }
}
