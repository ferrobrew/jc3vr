//! The `VR: CONTROLS` page's rows, grouped by header, in the order
//! `crate::controllers::menu::controls::settings_page` builds them.

use std::sync::atomic::Ordering;

use crate::{
    controllers::{
        config::LocomotionDirection,
        frame::Hand,
        menu::{config_choice, config_slider, config_toggle, controls::CONFIRMING_RESET},
        vehicles::config::{ArmPitch, WheelTracking},
    },
    pause_menu::model::{
        page::{Button, ButtonOutcome, Page},
        slider::{SliderFormat, SliderRange},
    },
};

/// The `CONTROLLERS` section: the hand and movement.
pub(super) fn controllers_section(page: Page) -> Page {
    page.header("CONTROLLERS")
        .item(config_toggle!("MOTION CONTROLLERS", |c| c
            .controllers
            .enabled))
        .item(config_choice!(DOMINANT_HAND, &HANDS, &HAND_LABELS, |c| c
            .controllers
            .dominant_hand))
        .text("A CHANGE OF HAND TAKES EFFECT")
        .text("WHEN THE GAME RESTARTS.")
        .item(config_choice!(
            "MOVE DIRECTION",
            &DIRECTIONS,
            &DIRECTION_LABELS,
            |c| c.controllers.locomotion.direction
        ))
        .item(config_slider!(
            "MOVE STICK DEADZONE",
            DEADZONE_RANGE,
            SliderFormat::new(2, ""),
            |c| c.controllers.bindings.move_deadzone
        ))
}

/// The `HANDS AND AIM` section.
pub(super) fn hands_and_aim_section(page: Page) -> Page {
    page.header("HANDS AND AIM")
        .item(config_slider!(
            "AIM ASSIST",
            UNIT_RANGE,
            SliderFormat::new(2, ""),
            |c| c.controllers.aim.assist_strength
        ))
        .item(config_toggle!("GRIP MARKERS", |c| c
            .controllers
            .overlay
            .markers))
        .item(config_slider!(
            "HAND STRENGTH",
            STRENGTH_RANGE,
            SliderFormat::new(2, "X"),
            |c| c.controllers.physical_hands.bodies.strength
        ))
}

/// The `WEAPONS` section.
pub(super) fn weapons_section(page: Page) -> Page {
    page.header("WEAPONS")
        .item(config_toggle!("HOLD GRIP TO HOLD GUNS", |c| c
            .controllers
            .weapons
            .hold
            .grip_to_hold))
        .item(config_toggle!("STICK CLICK CYCLES WEAPONS", |c| c
            .controllers
            .weapons
            .holsters
            .stick_click_cycles))
        .item(config_toggle!("TRUE RELOAD", |c| c
            .controllers
            .weapons
            .reload
            .enabled))
        .item(config_slider!(
            "RECOIL STRENGTH",
            RECOIL_RANGE,
            SliderFormat::new(2, "X"),
            |c| c.controllers.weapons.recoil.kick_scale
        ))
}

/// The `HAPTICS` section.
pub(super) fn haptics_section(page: Page) -> Page {
    page.header("HAPTICS")
        .item(config_toggle!("HAPTICS", |c| c.controllers.haptics.enabled))
        .item(config_slider!(
            "HAPTIC STRENGTH",
            UNIT_RANGE,
            SliderFormat::new(2, ""),
            |c| c.controllers.haptics.intensity
        ))
        .item(config_toggle!("GAME RUMBLE", |c| c
            .controllers
            .haptics
            .game_rumble))
}

/// The `VEHICLES` section, up to the wingsuit's pitch measure (whose calibration rows
/// `crate::controllers::menu::controls::pitch_rows` adds, between this section and
/// `parachute_and_saved_settings_section`).
pub(super) fn vehicles_section(page: Page) -> Page {
    page.header("VEHICLES")
        .item(config_choice!(
            "WHEEL TURNED BY",
            &WHEEL_TRACKING,
            &WHEEL_TRACKING_LABELS,
            |c| c.controllers.vehicles.physical.wheel.turned_by
        ))
        .item(config_slider!(
            "WHEEL FULL LOCK",
            WHEEL_LOCK_RANGE,
            SliderFormat::new(0, " DEG"),
            |c| c.controllers.vehicles.physical.wheel.lock_deg
        ))
        .item(config_toggle!("WINGSUIT ARM STEERING", |c| c
            .controllers
            .vehicles
            .arm_steering
            .wingsuit))
        .item(config_choice!(
            "WINGSUIT PITCH BY",
            &ArmPitch::ALL,
            &ARM_PITCH_LABELS,
            |c| c.controllers.vehicles.arm_steering.pitch_by
        ))
}

/// The rest of the `VEHICLES` section (the wingsuit's pitch inversion and the parachute handles)
/// and the `SAVED SETTINGS` section, after `crate::controllers::menu::controls::pitch_rows` has
/// added the wingsuit's calibration rows.
pub(super) fn parachute_and_saved_settings_section(page: Page) -> Page {
    page.item(config_toggle!("INVERT WINGSUIT PITCH", |c| c
        .controllers
        .vehicles
        .arm_steering
        .invert_pitch))
        .item(config_toggle!("PARACHUTE HANDLES", |c| c
            .controllers
            .vehicles
            .parachute_handles
            .enabled))
        .header("SAVED SETTINGS")
        .text("CHANGES APPLY AT ONCE AND ARE")
        .text("SAVED AUTOMATICALLY.")
        .item(Button {
            label: "RESET TO DEFAULTS".into(),
            caption: "RESET".into(),
            action: || {
                CONFIRMING_RESET.store(true, Ordering::Relaxed);
                ButtonOutcome::Rebuild
            },
        })
}

/// The dominant hand's row, which the reset leaves alone.
pub(super) const DOMINANT_HAND: &str = "DOMINANT HAND";

pub(super) const HANDS: [Hand; 2] = [Hand::Left, Hand::Right];
pub(super) const HAND_LABELS: [&str; 2] = ["LEFT", "RIGHT"];

pub(super) const DIRECTIONS: [LocomotionDirection; 2] =
    [LocomotionDirection::Head, LocomotionDirection::OffHand];
pub(super) const DIRECTION_LABELS: [&str; 2] = ["HEAD", "OFF HAND"];

pub(super) const WHEEL_TRACKING: [WheelTracking; 2] =
    [WheelTracking::Position, WheelTracking::Twist];
pub(super) const WHEEL_TRACKING_LABELS: [&str; 2] = ["HAND TRAVEL", "WRIST TWIST"];

/// By [`ArmPitch::ALL`].
pub(super) const ARM_PITCH_LABELS: [&str; 3] = ["WRIST TILT", "HAND HEIGHT", "ARM SWEEP"];

// Slider notch counts are multiples of ten: the movie's keyboard step is a tenth of the notches,
// truncated to an integer.

/// A strength or share, `0..=1` in hundredths.
pub(super) const UNIT_RANGE: SliderRange = SliderRange::new(0.0, 1.0, 100);
/// The move stick's deadzone, in hundredths.
pub(super) const DEADZONE_RANGE: SliderRange = SliderRange::new(0.0, 0.5, 50);
/// The recoil kick scale, in twentieths.
pub(super) const RECOIL_RANGE: SliderRange = SliderRange::new(0.0, 2.0, 40);
/// The steering wheel's full lock either way, degrees, in degrees: at most half a turn.
pub(super) const WHEEL_LOCK_RANGE: SliderRange = SliderRange::new(60.0, 180.0, 120);
/// The hands' strength scale, in twentieths.
pub(super) const STRENGTH_RANGE: SliderRange = SliderRange::new(0.0, 2.0, 40);
