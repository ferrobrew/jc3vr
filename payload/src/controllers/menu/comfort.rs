//! The `VR: COMFORT` page: recentering the view and steadying it on foot, the stick movement, the
//! view on the grapple, the posture, the body following the head, the lean it leaves alone, and
//! the stop at walls, crouching, the comfort fades, and the
//! view in vehicles: getting in, and the driver's view at the wheel. The recenter is an action, not
//! a setting.

use std::time::{Duration, Instant};

use parking_lot::Mutex;
use smol_str::SmolStr;

use crate::{
    controllers::{
        config::{Posture, SeatTransition},
        menu::{config_choice, config_slider, config_toggle},
        roomscale::manual_recenter_kind,
    },
    grapple::GrappleComfortMode,
    pause_menu::model::{
        page::{Button, ButtonOutcome, Page, PageDef, Readout},
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
        .item(config_toggle!("STEADY VIEW ON FOOT", |c| c
            .controllers
            .locomotion
            .view
            .enabled))
        .header("MOVEMENT")
        .item(config_toggle!("SMOOTH STICK MOVEMENT", |c| c
            .controllers
            .locomotion
            .stick
            .enabled))
        .item(config_slider!(
            "MOVE ACCELERATION",
            ACCELERATION_RANGE,
            SliderFormat::new(1, " M/S2"),
            |c| c.controllers.locomotion.stick.acceleration_mps2
        ))
        .item(config_slider!(
            "MOVE DECELERATION",
            ACCELERATION_RANGE,
            SliderFormat::new(1, " M/S2"),
            |c| c.controllers.locomotion.stick.deceleration_mps2
        ))
        .item(config_toggle!("NO TURNING ANIMATIONS", |c| c
            .controllers
            .locomotion
            .acts
            .enabled))
        .header("GRAPPLE")
        .item(config_choice!(
            "VIEW ON THE GRAPPLE",
            &GRAPPLE_MODES,
            &GRAPPLE_MODE_LABELS,
            |c| c.headpose.grapple.mode
        ))
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
        .item(config_toggle!("LEAN WITHOUT STEPPING", |c| c
            .controllers
            .body
            .rig
            .lean
            .enabled))
        .item(config_toggle!("STOP AT WALLS", |c| c
            .controllers
            .body
            .rig
            .pushback
            .enabled))
        .header("CROUCHING")
        .item(config_toggle!("CROUCH COLLISION", |c| c
            .controllers
            .roomscale
            .crouch
            .capsule))
        .item(config_slider!(
            "SEATED CROUCH DEPTH",
            SEATED_DROP_RANGE,
            SliderFormat::new(2, " M"),
            |c| c.controllers.roomscale.crouch.seated_drop_m
        ))
        .header("FADES")
        .item(config_toggle!("FADE WHEN HEAD IS IN WALLS", |c| c
            .controllers
            .roomscale
            .head_fade
            .enabled))
        .item(config_slider!(
            "FADE OUT TIME",
            FADE_TIME_RANGE,
            SliderFormat::new(2, " S"),
            |c| c.controllers.roomscale.fade.out_s
        ))
        .item(config_slider!(
            "FADE IN TIME",
            FADE_TIME_RANGE,
            SliderFormat::new(2, " S"),
            |c| c.controllers.roomscale.fade.in_s
        ))
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
        .item(config_toggle!("FIT VIEW IN BOATS TOO", |c| c
            .controllers
            .vehicles
            .driving_view
            .boats))
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

const GRAPPLE_MODES: [GrappleComfortMode; 3] = [
    GrappleComfortMode::HoldView,
    GrappleComfortMode::LevelPitch,
    GrappleComfortMode::Off,
];
const GRAPPLE_MODE_LABELS: [&str; 3] = ["HOLD STILL", "KEEP LEVEL", "SWING WITH BODY"];

// Slider notch counts are multiples of ten: the movie's keyboard step is a tenth of the notches,
// truncated to an integer.

/// The stick locomotion's rates, metres per second squared: from a slow, drifting start to all
/// but instant.
const ACCELERATION_RANGE: SliderRange = SliderRange::new(2.0, 42.0, 80);
/// How far the seated crouch lowers the eyes, in centimetres.
const SEATED_DROP_RANGE: SliderRange = SliderRange::new(0.2, 1.2, 100);
/// A fade's duration, in hundredths of a second.
const FADE_TIME_RANGE: SliderRange = SliderRange::new(0.0, 1.0, 100);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::menu::tests::exercise_page;

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
        assert_eq!(GRAPPLE_MODES.len(), GRAPPLE_MODE_LABELS.len());
    }
}
