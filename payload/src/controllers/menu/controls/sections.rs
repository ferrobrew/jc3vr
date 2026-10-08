//! The `VR: CONTROLS` page's rows, grouped by header, in the order
//! `crate::controllers::menu::controls::settings_page` builds them.

use std::sync::atomic::Ordering;

use smol_str::SmolStr;

use crate::{
    config::Config,
    controllers::{
        config::{GunReadout, LocomotionDirection, SchemeSetting},
        frame::Hand,
        menu::{
            config_choice, config_slider, config_toggle, controls::CONFIRMING_RESET, edit_config,
            nearest_index, read_config, settings_state,
        },
        vehicles::config::{ArmPitch, WheelTracking},
        weapons::{
            ads::config::AimingEye,
            config::{LeftGrip, PouchSide},
            reload::config::ReloadButton,
        },
    },
    headpose::config::{VrTurnConfig, VrTurnMode},
    pause_menu::model::{
        page::{Button, ButtonOutcome, Choice, Page, Readout, Toggle},
        slider::{SliderFormat, SliderRange},
    },
    vr::input::{
        self as vr_input, InputStatus,
        restart::{self, RestartReason},
    },
};

/// The `CONTROLLERS` section: the hand, the scheme, and movement.
pub(super) fn controllers_section(page: Page) -> Page {
    page.header("CONTROLLERS")
        .item(config_toggle!("MOTION CONTROLLERS", |c| c
            .controllers
            .enabled))
        .item(config_choice!(
            "CONTROL SCHEME",
            &SCHEMES,
            &SCHEME_LABELS,
            |c| c.controllers.scheme
        ))
        .item(config_choice!(DOMINANT_HAND, &HANDS, &HAND_LABELS, |c| c
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
}

/// The `TURNING` section.
pub(super) fn turning_section(page: Page) -> Page {
    page.header("TURNING")
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
}

/// The `HANDS AND AIM` section.
pub(super) fn hands_and_aim_section(page: Page) -> Page {
    page.header("HANDS AND AIM")
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
        .item(config_toggle!("FINGERS WRAP WHAT THE HAND HOLDS", |c| c
            .controllers
            .fingers
            .grip
            .enabled))
        .item(config_toggle!("FINGERS WRAP GUNS", |c| c
            .controllers
            .fingers
            .grip
            .weapons))
        .item(config_toggle!("PHYSICAL HANDS", |c| c
            .controllers
            .physical_hands
            .bodies
            .enabled))
        .item(config_slider!(
            "HAND STRENGTH",
            STRENGTH_RANGE,
            SliderFormat::new(2, "X"),
            |c| c.controllers.physical_hands.bodies.strength
        ))
        .item(config_toggle!("HAND BLOWS DEAL DAMAGE", |c| c
            .controllers
            .physical_hands
            .bodies
            .hits
            .damage))
        .item(config_toggle!("CLIMB WITH THE HANDS", |c| c
            .controllers
            .physical_hands
            .climb
            .enabled))
        .item(config_toggle!("CLIMB MOVING THINGS", |c| c
            .controllers
            .physical_hands
            .climb
            .moving))
        .item(config_toggle!("HOLD STUNT POSITIONS", |c| c
            .controllers
            .physical_hands
            .stunt
            .enabled))
        .item(config_toggle!("PUSH UP WITH THE HANDS", |c| c
            .controllers
            .physical_hands
            .vault
            .enabled))
        .item(config_toggle!("MOVE HINGED THINGS BY HAND", |c| c
            .controllers
            .physical_hands
            .pickup
            .articulated))
        .item(config_toggle!("OPEN DOORS BY HAND", |c| c
            .controllers
            .physical_hands
            .doors
            .enabled))
        .item(config_toggle!("GRAB CHARACTERS", |c| c
            .controllers
            .physical_hands
            .characters
            .grab))
        .item(config_toggle!("PULL OCCUPANTS OUT BY HAND", |c| c
            .controllers
            .physical_hands
            .characters
            .hijack))
        .item(config_toggle!("HAND IMPACT SOUNDS", |c| c
            .controllers
            .physical_hands
            .sounds
            .enabled))
}

/// The `GRAPPLE` section.
pub(super) fn grapple_section(page: Page) -> Page {
    page.header("GRAPPLE")
        .item(config_toggle!("YANK TO ZIP", |c| c
            .controllers
            .bindings
            .yank
            .enabled))
        .item(config_toggle!("PULL TO SPEED THE REEL", |c| c
            .controllers
            .grapple
            .reel
            .enabled))
        .item(config_slider!(
            "FASTEST PULLED REEL",
            REEL_SCALE_RANGE,
            SliderFormat::new(2, "X"),
            |c| c.controllers.grapple.reel.max_scale
        ))
        .item(config_toggle!("PULL TO TIGHTEN TETHERS", |c| c
            .controllers
            .grapple
            .tether
            .enabled))
        .item(config_toggle!("SWING FROM THE LINE BY HAND", |c| c
            .controllers
            .hands
            .anchor
            .hang_swing))
        .item(config_toggle!("PUMP THE SWING", |c| c
            .controllers
            .grapple
            .swing
            .pump))
}

/// The `CONTROLS SUMMARY` section.
pub(super) fn controls_summary_section(page: Page) -> Page {
    page.header("CONTROLS SUMMARY")
        .item(config_toggle!("CONTROLS ON WRIST", |c| c
            .controllers
            .summary
            .enabled))
        .item(config_toggle!("HIDE GAME BUTTON HINTS", |c| c
            .controllers
            .summary
            .hide_game_hints))
}

/// The `WEAPONS` section.
pub(super) fn weapons_section(page: Page) -> Page {
    page.header("WEAPONS")
        .item(config_toggle!("HOLSTERS", |c| c
            .controllers
            .weapons
            .holsters
            .enabled))
        .item(config_toggle!("HOLSTERS FOLLOW THE GUNS", |c| c
            .controllers
            .weapons
            .holsters
            .follow_docks))
        .item(config_toggle!("STICK CLICK CYCLES WEAPONS", |c| c
            .controllers
            .weapons
            .holsters
            .stick_click_cycles))
        .item(config_toggle!("WEAPON WHEEL", |c| c
            .controllers
            .weapons
            .wheel
            .enabled))
        .item(config_toggle!("TRIGGER DRAWS WEAPON", |c| c
            .controllers
            .weapons
            .fire_draws_weapon))
        .item(config_toggle!("SWITCH WEAPON WHEN EMPTY", |c| c
            .controllers
            .weapons
            .auto_switch_when_empty))
        .item(config_toggle!("EACH HAND FIRES ITS PISTOL", |c| c
            .controllers
            .weapons
            .dual_wield
            .split))
        .item(config_toggle!("HOLD WEAPONS IN EITHER HAND", |c| c
            .controllers
            .weapons
            .hold
            .either_hand))
        .item(config_toggle!("HAND GUNS OVER AT THE GRIP", |c| c
            .controllers
            .weapons
            .hold
            .handover))
        .item(config_toggle!("PICK WEAPONS UP INTO THE HAND", |c| c
            .controllers
            .weapons
            .hold
            .direct_pickup))
        .item(config_toggle!("DRAW LONG GUNS WITHOUT ANIMATION", |c| c
            .controllers
            .weapons
            .hold
            .instant_dock))
        .item(config_toggle!("DRAW PISTOLS WITHOUT ANIMATION", |c| c
            .controllers
            .weapons
            .hold
            .pistol_dock))
        .item(config_toggle!("HOLD GRIP TO HOLD GUNS", |c| c
            .controllers
            .weapons
            .hold
            .grip_to_hold))
        .item(config_toggle!("SECOND GUN IN THE FREE HAND", |c| c
            .controllers
            .weapons
            .hold
            .second_weapon))
        .item(config_toggle!("TAKE GUNS FROM HANDS", |c| c
            .controllers
            .weapons
            .hold
            .steal))
        .item(config_toggle!("TAKE WEAPONS FROM RACKS", |c| c
            .controllers
            .weapons
            .hold
            .racks))
        .item(config_toggle!("SLIDE TAKEN GUNS INTO THE HAND", |c| c
            .controllers
            .weapons
            .hold
            .take_slide))
        .item(config_choice!(
            "LEFT HAND GUN GRIP",
            &LeftGrip::ALL,
            &LEFT_GRIP_LABELS,
            |c| c.controllers.weapons.hold.left_grip
        ))
        .item(config_toggle!("LONE SECOND GUN BECOMES THE MAIN", |c| c
            .controllers
            .weapons
            .hold
            .promote_second))
        .item(config_toggle!("SECOND GUN'S AMMO OVER ITS HAND", |c| c
            .controllers
            .weapons
            .hold
            .magazine_pips))
        .item(config_toggle!("PICKUPS DROP THE OLD WEAPON", |c| c
            .controllers
            .weapons
            .hold
            .drop_replaced))
        .item(config_toggle!("TRACK WEAPONS", |c| c
            .controllers
            .weapons
            .inventory
            .enabled))
        .item(config_toggle!("KEEP DROPPED WEAPONS NEARBY", |c| c
            .controllers
            .weapons
            .inventory
            .pin_world))
        .item(config_toggle!("CARRY EXTRA WEAPONS", |c| c
            .controllers
            .weapons
            .inventory
            .carry))
        .item(config_toggle!("SAVE CARRIED WEAPONS", |c| c
            .controllers
            .weapons
            .inventory
            .persist))
        .item(config_toggle!("TWO-HANDED FOREGRIP", |c| c
            .controllers
            .weapons
            .foregrip
            .enabled))
        .item(config_toggle!("HOLD GRIP TO HOLD FOREGRIP", |c| c
            .controllers
            .weapons
            .foregrip
            .hold))
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
        .item(config_toggle!("ONE-HANDED WEIGHT", |c| c
            .controllers
            .weapons
            .heft
            .enabled))
        .item(config_toggle!("TWO-HANDED WEIGHT", |c| c
            .controllers
            .weapons
            .heft
            .braced
            .enabled))
        .item(config_slider!(
            "TWO-HANDED STRENGTH",
            WEIGHT_RANGE,
            SliderFormat::new(2, "X"),
            |c| c.controllers.weapons.heft.braced.strength
        ))
        .item(config_toggle!("SCOPES", |c| c.controllers.weapons.scopes))
        .item(config_toggle!("SIGHTS TO THE EYE", |c| c
            .controllers
            .weapons
            .ads
            .enabled))
        .item(config_choice!(
            "AIMING EYE",
            &AimingEye::ALL,
            &AIMING_EYE_LABELS,
            |c| c.controllers.weapons.ads.eye
        ))
        .item(config_toggle!("STOCK TO THE SHOULDER", |c| c
            .controllers
            .weapons
            .ads
            .shoulder))
        .item(config_toggle!("SCOPE CHEEK WELD", |c| c
            .controllers
            .weapons
            .ads
            .cheek_weld))
        .item(config_toggle!("GUNS STOP AT WALLS", |c| c
            .controllers
            .weapons
            .collide
            .enabled))
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
        .item(config_toggle!("MAGAZINES STAY IN THE WORLD", |c| c
            .controllers
            .weapons
            .reload
            .loose
            .enabled))
        .item(config_toggle!("CHAMBER BY HAND", |c| c
            .controllers
            .weapons
            .reload
            .chamber))
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
        .item(config_toggle!("PULL THE GRENADE PIN", |c| c
            .controllers
            .weapons
            .grenade
            .pin
            .enabled))
        .item(config_toggle!("CATCH LIVE GRENADES", |c| c
            .controllers
            .weapons
            .grenade
            .catch
            .enabled))
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
        .item(config_toggle!("PRESS C4 ONTO SURFACES", |c| c
            .controllers
            .weapons
            .c4
            .stick
            .enabled))
        .item(config_toggle!("WRIST DETONATOR", |c| c
            .controllers
            .weapons
            .wrist_detonator))
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
        .item(config_toggle!("CONTACT HAPTICS", |c| c
            .controllers
            .haptics
            .categories
            .contact))
        .item(config_toggle!("WEAPON HAPTICS", |c| c
            .controllers
            .haptics
            .categories
            .weapons))
        .item(config_toggle!("VEHICLE HAPTICS", |c| c
            .controllers
            .haptics
            .categories
            .vehicles))
        .item(config_toggle!("TRAVERSAL HAPTICS", |c| c
            .controllers
            .haptics
            .categories
            .traversal))
        .item(config_toggle!("CHORD HAPTICS", |c| c
            .controllers
            .haptics
            .categories
            .interface))
        .item(config_toggle!("GAME RUMBLE", |c| c
            .controllers
            .haptics
            .game_rumble))
        .item(config_toggle!("ALSO RUMBLE GAMEPAD", |c| c
            .controllers
            .haptics
            .gamepad_rumble))
        .item(config_toggle!("PER-CUE FREQUENCIES", |c| c
            .controllers
            .haptics
            .frequencies))
}

/// The `VEHICLES` section, up to the wingsuit's pitch measure (whose rows
/// `crate::controllers::menu::controls::pitch_rows` adds, between this section and
/// `parachute_and_saved_settings_section`).
pub(super) fn vehicles_section(page: Page) -> Page {
    page.header("VEHICLES")
        .item(config_toggle!("VEHICLE WEAPONS BY HAND", |c| c
            .controllers
            .vehicles
            .driver_weapons_by_hand))
        .item(config_toggle!("PASSENGER BODY AIMS BY HAND", |c| c
            .controllers
            .vehicles
            .passenger_body_by_hand))
        .item(config_toggle!("MOUNTED GUNS BY HAND", |c| c
            .controllers
            .vehicles
            .mounted
            .aim_by_hands))
        .item(config_toggle!("GRIP MOUNTED GUN HANDLES", |c| c
            .controllers
            .vehicles
            .mounted
            .grip_handles))
        .item(config_toggle!("MOUNTED GUN RECOIL", |c| c
            .controllers
            .vehicles
            .mounted
            .recoil))
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
        .item(config_slider!(
            "FLIGHT STICK THROW",
            STICK_THROW_RANGE,
            SliderFormat::new(2, " M"),
            |c| c
                .controllers
                .vehicles
                .physical
                .flight_stick
                .max_deflection_m
        ))
        .item(config_toggle!("TWIST FLIGHT STICK FOR RUDDER", |c| c
            .controllers
            .vehicles
            .physical
            .flight
            .twist_rudder))
        .item(config_toggle!("THROTTLE AFTERBURNER GATE", |c| c
            .controllers
            .vehicles
            .physical
            .flight
            .afterburner_gate))
        .item(config_toggle!("HORN ON THE WHEEL HUB", |c| c
            .controllers
            .vehicles
            .physical
            .buttons
            .horn))
        .item(config_toggle!("NITROUS BUTTON BY THE WHEEL", |c| c
            .controllers
            .vehicles
            .physical
            .buttons
            .nitrous_button))
        .item(config_toggle!("TWIST GRIP FOR NITROUS", |c| c
            .controllers
            .vehicles
            .physical
            .buttons
            .twist_nitrous))
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

/// The rest of the `VEHICLES` section (the wingsuit's other rows and the parachute
/// handles) and the `SAVED SETTINGS` section, after `crate::controllers::menu::controls::pitch_rows`
/// has added the wingsuit's pitch-measure rows.
pub(super) fn parachute_and_saved_settings_section(page: Page) -> Page {
    page.item(config_toggle!("INVERT WINGSUIT PITCH", |c| c
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

/// The button layout the controllers have, for a readout.
pub(super) fn button_layout() -> SmolStr {
    let status = vr_input::status();
    let configured = read_config(|c| c.controllers.dominant_hand);
    layout_label(&status, configured).into()
}

/// The button layout `status` reports for a player whose configured hand is `configured`.
pub(super) fn layout_label(status: &InputStatus, configured: Hand) -> &'static str {
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

/// The dominant hand's row, which the reset leaves alone.
pub(super) const DOMINANT_HAND: &str = "DOMINANT HAND";

pub(super) const SCHEMES: [SchemeSetting; 3] = [
    SchemeSetting::Auto,
    SchemeSetting::Controllers,
    SchemeSetting::Gamepad,
];
pub(super) const SCHEME_LABELS: [&str; 3] = ["AUTOMATIC", "CONTROLLERS", "GAMEPAD"];

pub(super) const HANDS: [Hand; 2] = [Hand::Left, Hand::Right];
/// By [`ReloadButton::ALL`].
pub(super) const RELOAD_BUTTON_LABELS: [&str; 2] = ["EJECT", "GAME RELOAD"];
/// By [`AimingEye::ALL`].
pub(super) const AIMING_EYE_LABELS: [&str; 3] = ["GUN HAND'S SIDE", "LEFT", "RIGHT"];
pub(super) const POUCH_SIDE_LABELS: [&str; 2] = ["GUN HAND SIDE", "FREE HAND SIDE"];
pub(super) const LEFT_GRIP_LABELS: [&str; 3] =
    ["AUTOMATIC", "LEFT HAND'S OWN", "RIGHT HAND'S MIRRORED"];
pub(super) const HAND_LABELS: [&str; 2] = ["LEFT", "RIGHT"];

pub(super) const DIRECTIONS: [LocomotionDirection; 2] =
    [LocomotionDirection::Head, LocomotionDirection::OffHand];
pub(super) const DIRECTION_LABELS: [&str; 2] = ["HEAD", "OFF HAND"];

pub(super) const TURN_MODES: [VrTurnMode; 2] = [VrTurnMode::Smooth, VrTurnMode::Snap];
pub(super) const TURN_MODE_LABELS: [&str; 2] = ["SMOOTH", "SNAP"];

/// The snap-turn steps offered, degrees. A saved angle between them shows as the nearest.
pub(super) const SNAP_ANGLES: [f32; 5] = [15.0, 30.0, 45.0, 60.0, 90.0];
pub(super) const SNAP_ANGLE_LABELS: [&str; 5] = [
    "15 DEGREES",
    "30 DEGREES",
    "45 DEGREES",
    "60 DEGREES",
    "90 DEGREES",
];

pub(super) const WHEEL_TRACKING: [WheelTracking; 2] =
    [WheelTracking::Position, WheelTracking::Twist];
pub(super) const WHEEL_TRACKING_LABELS: [&str; 2] = ["HAND TRAVEL", "WRIST TWIST"];

/// By [`ArmPitch::ALL`].
pub(super) const ARM_PITCH_LABELS: [&str; 3] = ["WRIST TILT", "HAND HEIGHT", "ARM SWEEP"];

pub(super) const GUN_READOUTS: [GunReadout; 3] =
    [GunReadout::None, GunReadout::Dot, GunReadout::Laser];
pub(super) const GUN_READOUT_LABELS: [&str; 3] = ["NONE", "DOT", "LASER"];

// Slider notch counts are multiples of ten: the movie's keyboard step is a tenth of the notches,
// truncated to an integer.

/// A strength or share, `0..=1` in hundredths.
pub(super) const UNIT_RANGE: SliderRange = SliderRange::new(0.0, 1.0, 100);
/// The move stick's deadzone, in hundredths.
pub(super) const DEADZONE_RANGE: SliderRange = SliderRange::new(0.0, 0.5, 50);
/// The smooth-turn scale, in twentieths.
pub(super) const TURN_SPEED_RANGE: SliderRange = SliderRange::new(0.5, 3.0, 50);
/// The recoil kick scale, in twentieths.
pub(super) const RECOIL_RANGE: SliderRange = SliderRange::new(0.0, 2.0, 40);
/// The two-handed weight's strength, in twentieths.
pub(super) const WEIGHT_RANGE: SliderRange = SliderRange::new(0.0, 2.0, 40);
/// The parachute handles' full pull, metres, in centimetres.
pub(super) const FULL_PULL_RANGE: SliderRange = SliderRange::new(0.1, 0.6, 50);
/// The parachute handles' height above the shoulders, metres, in centimetres.
pub(super) const HANDLE_HEIGHT_RANGE: SliderRange = SliderRange::new(0.0, 0.4, 40);
/// The steering wheel's full lock either way, degrees, in degrees: at most half a turn.
pub(super) const WHEEL_LOCK_RANGE: SliderRange = SliderRange::new(60.0, 180.0, 120);
/// The flight stick's full deflection, metres, in half centimetres.
pub(super) const STICK_THROW_RANGE: SliderRange = SliderRange::new(0.05, 0.2, 30);
/// The steering wheel's response exponent, in twentieths.
pub(super) const WHEEL_RESPONSE_RANGE: SliderRange = SliderRange::new(0.5, 3.0, 50);

/// The hands' strength scale, in twentieths.
pub(super) const STRENGTH_RANGE: SliderRange = SliderRange::new(0.0, 2.0, 40);
/// The pulled reel's top scale, in twentieths.
pub(super) const REEL_SCALE_RANGE: SliderRange = SliderRange::new(1.0, 2.5, 30);

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
