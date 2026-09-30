//! The `VR: COMFORT` page: the posture and the roomscale chase, crouching, the comfort fades, and
//! how the view behaves getting into a vehicle. Only settings `controllers.local.json` holds are
//! here, so everything the page changes is saved when the player leaves it.

use crate::{
    controllers::{
        config::{Posture, SeatTransition},
        menu::{config_choice, config_slider, config_toggle, on_frame, save_if_changed},
    },
    pause_menu::model::{
        page::{Page, PageDef},
        slider::{SliderFormat, SliderRange},
    },
};

/// The `VR: COMFORT` page.
pub(crate) const PAGE: PageDef = PageDef {
    entry: "VR: COMFORT",
    build,
    on_close: Some(save_if_changed),
    on_frame: Some(on_frame),
};

fn build() -> Page {
    Page::new("VR: COMFORT")
        .header("POSTURE")
        .item(config_choice!("POSTURE", &POSTURES, &POSTURE_LABELS, |c| c
            .controllers
            .roomscale
            .posture))
        .item(config_toggle!("WALK IN THE ROOM", |c| c
            .controllers
            .roomscale
            .chase
            .enabled))
        .item(config_slider!(
            "ROOM WALK DEADZONE",
            CHASE_DEADZONE_RANGE,
            SliderFormat::new(2, " M"),
            |c| c.controllers.roomscale.chase.deadzone_m
        ))
        .header("CROUCHING")
        .item(config_toggle!("CROUCH WITH YOUR HEAD", |c| c
            .controllers
            .roomscale
            .crouch
            .visual))
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
        .item(config_choice!(
            "VEHICLE ENTRY",
            &SEAT_TRANSITIONS,
            &SEAT_TRANSITION_LABELS,
            |c| c.controllers.roomscale.seat_transition
        ))
}

const POSTURES: [Posture; 4] = [
    Posture::Auto,
    Posture::Seated,
    Posture::Standing,
    Posture::Roomscale,
];
const POSTURE_LABELS: [&str; 4] = ["AUTOMATIC", "SEATED", "STANDING", "ROOMSCALE"];

const SEAT_TRANSITIONS: [SeatTransition; 3] = [
    SeatTransition::Ride,
    SeatTransition::Fade,
    SeatTransition::Instant,
];
const SEAT_TRANSITION_LABELS: [&str; 3] = ["FOLLOW THE ANIMATION", "FADE", "INSTANT"];

// Slider notch counts are multiples of ten: the movie's keyboard step is a tenth of the notches,
// truncated to an integer.

/// How far the head may lead the body before the room walk follows, in centimetres.
const CHASE_DEADZONE_RANGE: SliderRange = SliderRange::new(0.0, 0.5, 50);
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
    fn the_choice_lists_line_up() {
        assert_eq!(POSTURES.len(), POSTURE_LABELS.len());
        assert_eq!(SEAT_TRANSITIONS.len(), SEAT_TRANSITION_LABELS.len());
    }
}
