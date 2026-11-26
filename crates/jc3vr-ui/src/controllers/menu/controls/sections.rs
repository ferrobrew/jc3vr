//! The `VR: CONTROLS` page's rows, grouped by header, in the order
//! `crate::controllers::menu::controls::settings_page` builds them.

use std::sync::atomic::Ordering;

use jc3vr_gameplay::controllers::{
    config::{CONTROLLERS_CONFIG, LocomotionDirection},
    vehicles::config::{ArmPitch, WheelTracking},
};
use jc3vr_input::frame::Hand;

use crate::{
    controllers::menu::{config_choice, config_slider, config_toggle, controls::CONFIRMING_RESET},
    pause_menu::model::{
        page::{Button, ButtonOutcome, Page},
        slider::{SliderFormat, SliderRange},
    },
};

/// The `CONTROLLERS` section: the hand and movement.
pub(super) fn controllers_section(page: Page) -> Page {
    page.header("CONTROLLERS")
        .item(config_toggle!(
            "MOTION CONTROLLERS",
            CONTROLLERS_CONFIG,
            |c| c.enabled
        ))
        .item(config_choice!(
            DOMINANT_HAND,
            &HANDS,
            &HAND_LABELS,
            CONTROLLERS_CONFIG,
            |c| c.dominant_hand
        ))
        .text("A CHANGE OF HAND TAKES EFFECT")
        .text("WHEN THE GAME RESTARTS.")
        .item(config_choice!(
            "MOVE DIRECTION",
            &DIRECTIONS,
            &DIRECTION_LABELS,
            CONTROLLERS_CONFIG,
            |c| c.locomotion.direction
        ))
        .item(config_slider!(
            "MOVE STICK DEADZONE",
            DEADZONE_RANGE,
            SliderFormat::new(2, ""),
            CONTROLLERS_CONFIG,
            |c| c.bindings.move_deadzone
        ))
}

/// The `HANDS AND AIM` section.
pub(super) fn hands_and_aim_section(page: Page) -> Page {
    page.header("HANDS AND AIM")
        .item(config_slider!(
            "AIM ASSIST",
            UNIT_RANGE,
            SliderFormat::new(2, ""),
            CONTROLLERS_CONFIG,
            |c| c.aim.assist_strength
        ))
        .item(config_toggle!("GRIP MARKERS", CONTROLLERS_CONFIG, |c| c
            .overlay
            .markers))
        .item(config_slider!(
            "HAND STRENGTH",
            STRENGTH_RANGE,
            SliderFormat::new(2, "X"),
            CONTROLLERS_CONFIG,
            |c| c.physical_hands.bodies.strength
        ))
}

/// The `WEAPONS` section.
pub(super) fn weapons_section(page: Page) -> Page {
    page.header("WEAPONS")
        .item(config_toggle!(
            "HOLD GRIP TO HOLD GUNS",
            CONTROLLERS_CONFIG,
            |c| c.weapons.hold.grip_to_hold
        ))
        .item(config_toggle!(
            "STICK CLICK CYCLES WEAPONS",
            CONTROLLERS_CONFIG,
            |c| c.weapons.holsters.stick_click_cycles
        ))
        .item(config_toggle!("TRUE RELOAD", CONTROLLERS_CONFIG, |c| c
            .weapons
            .reload
            .enabled))
        .item(config_slider!(
            "RECOIL STRENGTH",
            RECOIL_RANGE,
            SliderFormat::new(2, "X"),
            CONTROLLERS_CONFIG,
            |c| c.weapons.recoil.kick_scale
        ))
}

/// The `HAPTICS` section.
pub(super) fn haptics_section(page: Page) -> Page {
    page.header("HAPTICS")
        .item(config_toggle!("HAPTICS", CONTROLLERS_CONFIG, |c| c
            .haptics
            .enabled))
        .item(config_slider!(
            "HAPTIC STRENGTH",
            UNIT_RANGE,
            SliderFormat::new(2, ""),
            CONTROLLERS_CONFIG,
            |c| c.haptics.intensity
        ))
        .item(config_toggle!("GAME RUMBLE", CONTROLLERS_CONFIG, |c| c
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
            CONTROLLERS_CONFIG,
            |c| c.vehicles.physical.wheel.turned_by
        ))
        .item(config_slider!(
            "WHEEL FULL LOCK",
            WHEEL_LOCK_RANGE,
            SliderFormat::new(0, " DEG"),
            CONTROLLERS_CONFIG,
            |c| c.vehicles.physical.wheel.lock_deg
        ))
        .item(config_toggle!(
            "WINGSUIT ARM STEERING",
            CONTROLLERS_CONFIG,
            |c| c.vehicles.arm_steering.wingsuit
        ))
        .item(config_choice!(
            "WINGSUIT PITCH BY",
            &ArmPitch::ALL,
            &ARM_PITCH_LABELS,
            CONTROLLERS_CONFIG,
            |c| c.vehicles.arm_steering.pitch_by
        ))
}

/// The rest of the `VEHICLES` section (the wingsuit's pitch inversion and the parachute handles)
/// and the `SAVED SETTINGS` section, after `crate::controllers::menu::controls::pitch_rows` has
/// added the wingsuit's calibration rows.
pub(super) fn parachute_and_saved_settings_section(page: Page) -> Page {
    page.item(config_toggle!(
        "INVERT WINGSUIT PITCH",
        CONTROLLERS_CONFIG,
        |c| c.vehicles.arm_steering.invert_pitch
    ))
    .item(config_toggle!(
        "PARACHUTE HANDLES",
        CONTROLLERS_CONFIG,
        |c| c.vehicles.parachute_handles.enabled
    ))
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
