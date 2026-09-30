//! The `VR: CONTROLS` page: the hand, the scheme, turning, the controller subsystems a player turns
//! on and off, their few tunings worth changing in the headset, and the saved settings (reload, and
//! reset to defaults behind a confirmation). The rest of [`ControllersConfig`] stays in the debug
//! UI.
//!
//! [`ControllersConfig`]: crate::controllers::config::ControllersConfig

use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    config::Config,
    controllers::{
        config::{GunReadout, LocomotionDirection, SchemeSetting},
        frame::Hand,
        menu::{
            config_choice, config_slider, config_toggle, edit_config, file_state, nearest_index,
            on_frame, read_config, reload_saved, reset_live_to_defaults, save_if_changed,
        },
    },
    headpose::config::{VrTurnConfig, VrTurnMode},
    pause_menu::model::{
        page::{Button, ButtonOutcome, Choice, Page, PageDef, Readout, Toggle},
        slider::{SliderFormat, SliderRange},
    },
};

/// The `VR: CONTROLS` page.
pub(crate) const PAGE: PageDef = PageDef {
    entry: "VR: CONTROLS",
    build,
    on_close: Some(on_close),
    on_frame: Some(on_frame),
};

fn build() -> Page {
    if CONFIRMING_RESET.load(Ordering::Relaxed) {
        return confirm_reset();
    }
    let page = Page::new("VR: CONTROLS")
        .header("CONTROLLERS")
        .item(config_toggle!("MOTION CONTROLLERS", |c| c
            .controllers
            .enabled))
        .item(config_choice!(
            "CONTROL SCHEME",
            &SCHEMES,
            &SCHEME_LABELS,
            |c| c.controllers.scheme
        ))
        .item(config_choice!("DOMINANT HAND", &HANDS, &HAND_LABELS, |c| c
            .controllers
            .dominant_hand))
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
            label: "SNAP TURN ANGLE".into(),
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
        .header("HANDS AND AIM")
        .item(config_toggle!("HANDS FOLLOW CONTROLLERS", |c| c
            .controllers
            .hands
            .ik))
        .item(config_toggle!("AIM GUNS WITH HAND", |c| c
            .controllers
            .aim
            .gun))
        .item(config_toggle!("AIM GRAPPLE WITH HAND", |c| c
            .controllers
            .aim
            .grapple))
        .item(config_slider!(
            "AIM ASSIST",
            UNIT_RANGE,
            SliderFormat::new(2, ""),
            |c| c.controllers.aim.assist_strength
        ))
        .item(config_choice!(
            "GUN AIM MARKER",
            &GUN_READOUTS,
            &GUN_READOUT_LABELS,
            |c| c.controllers.overlay.gun_readout
        ))
        .item(config_toggle!("FINGERS", |c| c.controllers.fingers.enabled))
        .header("WEAPONS")
        .item(config_toggle!("HOLSTERS", |c| c
            .controllers
            .weapons
            .holsters
            .enabled))
        .item(config_toggle!("TWO-HANDED FOREGRIP", |c| c
            .controllers
            .weapons
            .foregrip
            .enabled))
        .item(config_toggle!("RECOIL", |c| c
            .controllers
            .weapons
            .recoil
            .enabled))
        .item(config_slider!(
            "RECOIL STRENGTH",
            RECOIL_RANGE,
            SliderFormat::new(2, "X"),
            |c| c.controllers.weapons.recoil.kick_scale
        ))
        .item(config_toggle!("SCOPES", |c| c.controllers.weapons.scopes))
        .item(config_toggle!("MAGAZINE RELOAD", |c| c
            .controllers
            .weapons
            .reload_gesture))
        .item(config_toggle!("THROWN GRENADES", |c| c
            .controllers
            .weapons
            .grenade_throw))
        .item(config_toggle!("MELEE STRIKES", |c| c
            .controllers
            .weapons
            .melee))
        .header("HAPTICS")
        .item(config_toggle!("HAPTICS", |c| c
            .controllers
            .weapons
            .haptics
            .enabled))
        .item(config_slider!(
            "HAPTIC STRENGTH",
            UNIT_RANGE,
            SliderFormat::new(2, ""),
            |c| c.controllers.weapons.haptics.intensity
        ))
        .item(config_toggle!("GAME RUMBLE", |c| c
            .controllers
            .weapons
            .haptics
            .game_rumble));
    page.header("VEHICLES")
        .item(config_toggle!("VEHICLE WEAPONS BY HAND", |c| c
            .controllers
            .vehicles
            .driver_weapons_by_hand))
        .item(config_toggle!("MOUNTED GUNS BY HAND", |c| c
            .controllers
            .vehicles
            .mounted
            .aim_by_hands))
        .item(Toggle {
            label: "PHYSICAL VEHICLE CONTROLS".into(),
            get: || read_config(|c| physical_controls(c).iter().all(|&&enabled| enabled)),
            set: |enabled| edit_config(|c| set_physical_controls(c, enabled)),
            default: physical_controls(&Config::new())
                .iter()
                .all(|&&enabled| enabled),
        })
        .item(config_toggle!("WINGSUIT ARM STEERING", |c| c
            .controllers
            .vehicles
            .arm_steering
            .wingsuit))
        .item(config_toggle!("PARACHUTE ARM STEERING", |c| c
            .controllers
            .vehicles
            .arm_steering
            .parachute))
        .header("SAVED SETTINGS")
        .text("CHANGES APPLY AT ONCE AND ARE SAVED")
        .text("WHEN YOU LEAVE A VR PAGE.")
        .item(Readout {
            label: "SETTINGS FILE".into(),
            read: file_state,
        })
        .item(Button {
            label: "RELOAD SAVED SETTINGS".into(),
            caption: "RELOAD".into(),
            action: reload_saved,
        })
        .item(Button {
            label: "RESET TO DEFAULTS".into(),
            caption: "RESET".into(),
            action: || {
                CONFIRMING_RESET.store(true, Ordering::Relaxed);
                ButtonOutcome::Rebuild
            },
        })
}

/// The reset confirmation, in place of the page's rows. Keeping the settings is the first button,
/// so it is the one focused.
fn confirm_reset() -> Page {
    Page::new("VR: CONTROLS")
        .header("RESET TO DEFAULTS")
        .text("EVERY CONTROLLER, TURNING, AND COMFORT")
        .text("SETTING RETURNS TO ITS DEFAULT.")
        .text("WEAPON SIGHT CALIBRATION IS KEPT.")
        .item(Button {
            label: "KEEP MY SETTINGS".into(),
            caption: "CANCEL".into(),
            action: || {
                CONFIRMING_RESET.store(false, Ordering::Relaxed);
                ButtonOutcome::Rebuild
            },
        })
        .item(Button {
            label: "RESET EVERYTHING".into(),
            caption: "RESET".into(),
            action: || {
                reset_live_to_defaults();
                CONFIRMING_RESET.store(false, Ordering::Relaxed);
                ButtonOutcome::Rebuild
            },
        })
}

fn on_close() {
    CONFIRMING_RESET.store(false, Ordering::Relaxed);
    save_if_changed();
}

/// Whether the page shows the reset confirmation.
static CONFIRMING_RESET: AtomicBool = AtomicBool::new(false);

const SCHEMES: [SchemeSetting; 3] = [
    SchemeSetting::Auto,
    SchemeSetting::Controllers,
    SchemeSetting::Gamepad,
];
const SCHEME_LABELS: [&str; 3] = ["AUTOMATIC", "CONTROLLERS", "GAMEPAD"];

const HANDS: [Hand; 2] = [Hand::Left, Hand::Right];
const HAND_LABELS: [&str; 2] = ["LEFT", "RIGHT"];

const DIRECTIONS: [LocomotionDirection; 2] =
    [LocomotionDirection::Head, LocomotionDirection::OffHand];
const DIRECTION_LABELS: [&str; 2] = ["HEAD", "OFF HAND"];

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

const GUN_READOUTS: [GunReadout; 3] = [GunReadout::None, GunReadout::Dot, GunReadout::Laser];
const GUN_READOUT_LABELS: [&str; 3] = ["NONE", "DOT", "LASER"];

// Slider notch counts are multiples of ten: the movie's keyboard step is a tenth of the notches,
// truncated to an integer.

/// A strength or share, `0..=1` in hundredths.
const UNIT_RANGE: SliderRange = SliderRange::new(0.0, 1.0, 100);
/// The move stick's deadzone, in hundredths.
const DEADZONE_RANGE: SliderRange = SliderRange::new(0.0, 0.5, 50);
/// The smooth-turn scale, in twentieths.
const TURN_SPEED_RANGE: SliderRange = SliderRange::new(0.5, 3.0, 50);
/// The recoil kick scale, in twentieths.
const RECOIL_RANGE: SliderRange = SliderRange::new(0.0, 2.0, 40);

/// The switches of the six physical vehicle controls.
fn physical_controls(c: &Config) -> [&bool; 6] {
    let p = &c.controllers.vehicles.physical;
    [
        &p.wheel.enabled,
        &p.handlebars.enabled,
        &p.cyclic.enabled,
        &p.collective.enabled,
        &p.flight_stick.enabled,
        &p.throttle.enabled,
    ]
}

fn set_physical_controls(c: &mut Config, enabled: bool) {
    let p = &mut c.controllers.vehicles.physical;
    for control in [
        &mut p.wheel.enabled,
        &mut p.handlebars.enabled,
        &mut p.cyclic.enabled,
        &mut p.collective.enabled,
        &mut p.flight_stick.enabled,
        &mut p.throttle.enabled,
    ] {
        *control = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        controllers::menu::tests::exercise_page,
        pause_menu::model::{page::Item, rows::encode_page},
    };

    #[test]
    fn the_page_edits_valid_settings_and_resets_to_the_defaults() {
        exercise_page(&build());
    }

    #[test]
    fn the_choice_lists_line_up() {
        assert_eq!(SCHEMES.len(), SCHEME_LABELS.len());
        assert_eq!(HANDS.len(), HAND_LABELS.len());
        assert_eq!(DIRECTIONS.len(), DIRECTION_LABELS.len());
        assert_eq!(TURN_MODES.len(), TURN_MODE_LABELS.len());
        assert_eq!(SNAP_ANGLES.len(), SNAP_ANGLE_LABELS.len());
        assert_eq!(GUN_READOUTS.len(), GUN_READOUT_LABELS.len());
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
            .find(|item| matches!(item, Item::Choice(c) if c.label == "SNAP TURN ANGLE"))
        else {
            panic!("no snap angle choice");
        };
        edit_config(|c| c.headpose.vr_turn.snap_angle_deg = 40.0);
        assert_eq!((snap.get)(), 2);
    }

    #[test]
    fn the_confirmation_keeps_the_settings_first() {
        let page = confirm_reset();
        let first = page.first_button().unwrap();
        let Item::Button(keep) = &page.items[first] else {
            panic!("the first button is not a button");
        };
        assert_eq!(keep.caption, "CANCEL");
        encode_page(0, &page).unwrap();
    }
}
