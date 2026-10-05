//! The `VR: CONTROLS` page: the hand, the scheme, turning, the controller subsystems a player turns
//! on and off, the HUD parts drawn at the hands and in the world, their few tunings worth changing
//! in the headset, and the saved settings (their state,
//! and a reset to defaults behind a confirmation). The rest of [`ControllersConfig`] stays in the debug
//! UI.
//!
//! [`ControllersConfig`]: crate::controllers::config::ControllersConfig

use std::sync::atomic::{AtomicBool, Ordering};

use smol_str::SmolStr;

use crate::{
    config::Config,
    controllers::{
        config::{GunReadout, LocomotionDirection, SchemeSetting},
        frame::Hand,
        menu::{
            config_choice, config_slider, config_toggle, edit_config, nearest_index, read_config,
            reset_live_to_defaults, settings_state,
        },
        vehicles::config::{ArmPitch, WheelTracking},
        weapons::{config::PouchSide, reload::config::ReloadButton},
    },
    headpose::config::{VrTurnConfig, VrTurnMode},
    pause_menu::model::{
        page::{Button, ButtonOutcome, Choice, Page, PageDef, Readout, Toggle},
        slider::{SliderFormat, SliderRange},
    },
    vr::input::{
        self as vr_input, InputStatus,
        restart::{self, RestartReason},
    },
};

/// The `VR: CONTROLS` page.
pub(crate) const PAGE: PageDef = PageDef {
    tab: "CONTROLS",
    build,
    on_close: Some(on_close),
    on_frame: None,
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
        .item(Readout {
            label: "BUTTON LAYOUT".into(),
            read: button_layout,
        })
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
        .item(config_toggle!("HIDE GAME CROSSHAIR", |c| c
            .controllers
            .overlay
            .hide_game_reticle))
        .item(config_toggle!("FINGERS", |c| c.controllers.fingers.enabled))
        .header("HUD")
        .item(config_toggle!("HUD ON THE HANDS", |c| c.hud.atlas.enabled))
        .item(config_toggle!("AMMO ABOVE GUN HAND", |c| c
            .hud
            .atlas
            .ammo
            .enabled))
        .item(config_toggle!("TETHERS ABOVE FOREARM", |c| c
            .hud
            .atlas
            .tether
            .enabled))
        .item(config_toggle!("MARKERS IN THE WORLD", |c| c
            .hud
            .world_markers
            .enabled))
        .header("CONTROLS SUMMARY")
        .item(config_toggle!("CONTROLS ON WRIST", |c| c
            .controllers
            .summary
            .enabled))
        .item(config_toggle!("HIDE GAME BUTTON HINTS", |c| c
            .controllers
            .summary
            .hide_game_hints))
        .header("WEAPONS")
        .item(config_toggle!("HOLSTERS", |c| c
            .controllers
            .weapons
            .holsters
            .enabled))
        .item(config_toggle!("STICK CLICK CYCLES WEAPONS", |c| c
            .controllers
            .weapons
            .holsters
            .stick_click_cycles))
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
        .item(config_toggle!("TRUE RELOAD", |c| c
            .controllers
            .weapons
            .reload
            .enabled))
        .item(config_choice!(
            "RELOAD BUTTON",
            &ReloadButton::ALL,
            &RELOAD_BUTTON_LABELS,
            |c| c.controllers.weapons.reload.button
        ))
        .item(config_toggle!("FLICK TO EJECT", |c| c
            .controllers
            .weapons
            .reload
            .flick_eject))
        .item(config_toggle!("MANUAL BOLT AND PUMP", |c| c
            .controllers
            .weapons
            .reload
            .manual_action))
        .item(config_toggle!("SHOW POUCHES", |c| c
            .controllers
            .weapons
            .pouches
            .show))
        .item(config_toggle!("THROWN GRENADES", |c| c
            .controllers
            .weapons
            .grenade_throw))
        .item(config_choice!(
            "GRENADE POUCH",
            &PouchSide::ALL,
            &POUCH_SIDE_LABELS,
            |c| c.controllers.weapons.pouches.grenade_side
        ))
        .item(config_toggle!("THROWN C4", |c| c
            .controllers
            .weapons
            .c4_throw))
        .item(config_toggle!("WRIST DETONATOR", |c| c
            .controllers
            .weapons
            .wrist_detonator))
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
            .game_rumble))
        .item(config_toggle!("ALSO RUMBLE GAMEPAD", |c| c
            .controllers
            .weapons
            .haptics
            .gamepad_rumble));
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
        .item(config_slider!(
            "WHEEL RESPONSE",
            WHEEL_RESPONSE_RANGE,
            SliderFormat::new(2, ""),
            |c| c.controllers.vehicles.physical.wheel.response
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
        .item(config_slider!(
            "WINGSUIT FULL TILT",
            TILT_RANGE,
            SliderFormat::new(0, " DEG"),
            |c| c.controllers.vehicles.arm_steering.full_tilt_deg
        ))
        .item(config_toggle!("INVERT WINGSUIT PITCH", |c| c
            .controllers
            .vehicles
            .arm_steering
            .invert_pitch))
        .item(config_toggle!("WINGSUIT ARMS READOUT", |c| c
            .controllers
            .vehicles
            .arm_steering
            .show_readout))
        .item(config_toggle!("PARACHUTE HANDLES", |c| c
            .controllers
            .vehicles
            .parachute_handles
            .enabled))
        .item(config_toggle!("MARK PARACHUTE HANDLES", |c| c
            .controllers
            .vehicles
            .parachute_handles
            .show_handles))
        .item(config_slider!(
            "HANDLE HEIGHT ABOVE SHOULDER",
            HANDLE_HEIGHT_RANGE,
            SliderFormat::new(2, " M"),
            |c| c.controllers.vehicles.parachute_handles.from_shoulder_m.y
        ))
        .item(config_slider!(
            "HANDLE FULL PULL",
            FULL_PULL_RANGE,
            SliderFormat::new(2, " M"),
            |c| c.controllers.vehicles.parachute_handles.full_pull_m
        ))
        .item(config_slider!(
            "HANDLE BRAKE THRESHOLD",
            UNIT_RANGE,
            SliderFormat::new(2, ""),
            |c| c.controllers.vehicles.parachute_handles.brake_threshold
        ))
        .header("SAVED SETTINGS")
        .text("CHANGES APPLY AT ONCE AND ARE")
        .text("SAVED AUTOMATICALLY.")
        .item(Readout {
            label: "SETTINGS FILE".into(),
            read: settings_state,
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
}

/// The button layout the controllers have, for a readout.
fn button_layout() -> SmolStr {
    let status = vr_input::status();
    let configured = read_config(|c| c.controllers.dominant_hand);
    layout_label(&status, configured).into()
}

/// The button layout `status` reports for a player whose configured hand is `configured`.
fn layout_label(status: &InputStatus, configured: Hand) -> &'static str {
    match restart::reason(status, configured) {
        Some(reason) if reason.blocks_input() => "RESTART THE GAME",
        Some(RestartReason::HandChanged { suggested }) => match suggested {
            Hand::Left => "LEFT-HANDED UNTIL RESTART",
            Hand::Right => "RIGHT-HANDED UNTIL RESTART",
        },
        _ => match status.suggested_for {
            Some(Hand::Left) => "LEFT-HANDED",
            Some(Hand::Right) => "RIGHT-HANDED",
            None => "NOT STARTED",
        },
    }
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
/// By [`ReloadButton::ALL`].
const RELOAD_BUTTON_LABELS: [&str; 2] = ["EJECT", "GAME RELOAD"];
const POUCH_SIDE_LABELS: [&str; 2] = ["DOMINANT SIDE", "OFF-HAND SIDE"];
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

const WHEEL_TRACKING: [WheelTracking; 2] = [WheelTracking::Position, WheelTracking::Twist];
const WHEEL_TRACKING_LABELS: [&str; 2] = ["HAND TRAVEL", "WRIST TWIST"];

/// By [`ArmPitch::ALL`].
const ARM_PITCH_LABELS: [&str; 3] = ["WRIST TILT", "HAND HEIGHT", "ARM SWEEP"];

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
/// The parachute handles' full pull, metres, in centimetres.
const FULL_PULL_RANGE: SliderRange = SliderRange::new(0.1, 0.6, 50);
/// The parachute handles' height above the shoulders, metres, in centimetres.
const HANDLE_HEIGHT_RANGE: SliderRange = SliderRange::new(0.0, 0.4, 40);
/// The steering wheel's full lock either way, degrees, in degrees: at most half a turn.
const WHEEL_LOCK_RANGE: SliderRange = SliderRange::new(60.0, 180.0, 120);
/// The steering wheel's response exponent, in twentieths.
const WHEEL_RESPONSE_RANGE: SliderRange = SliderRange::new(0.5, 3.0, 50);
/// The wingsuit's full pitch by wrist tilt, degrees, in degrees.
const TILT_RANGE: SliderRange = SliderRange::new(10.0, 60.0, 50);

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
        assert_eq!(WHEEL_TRACKING.len(), WHEEL_TRACKING_LABELS.len());
        assert_eq!(ArmPitch::ALL.len(), ARM_PITCH_LABELS.len());
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
    fn the_button_layout_says_when_a_restart_is_due() {
        let attached = |suggested| InputStatus {
            attached: true,
            suggested_for: Some(suggested),
            ..InputStatus::default()
        };
        assert_eq!(
            layout_label(&InputStatus::default(), Hand::Right),
            "NOT STARTED"
        );
        assert_eq!(
            layout_label(&attached(Hand::Left), Hand::Left),
            "LEFT-HANDED"
        );
        assert_eq!(
            layout_label(&attached(Hand::Right), Hand::Left),
            "RIGHT-HANDED UNTIL RESTART"
        );
        let failed = InputStatus {
            restart: Some(RestartReason::StashedForOtherHand),
            ..InputStatus::default()
        };
        assert_eq!(layout_label(&failed, Hand::Left), "RESTART THE GAME");
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
