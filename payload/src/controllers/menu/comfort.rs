//! The `VR: COMFORT` page: recentering the view, the posture, the body following the head, the
//! fade when the head is in a wall, turning, the stick movement, and the view in vehicles: getting
//! in, and the driver's view at the wheel. The recenter is an action, not a setting. The comfort
//! tuning (the fades' times, the crouch, the lean, the stick's rates, the view on the grapple) is
//! in the debug UI.

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
        page::{Button, ButtonOutcome, Choice, Page, PageDef},
        slider::{SliderFormat, SliderRange},
    },
    vr::request_recenter,
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
        .text("LOOK AHEAD, THEN PRESS RECENTER.")
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
    request_recenter(manual_recenter_kind());
    ButtonOutcome::Stay
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
