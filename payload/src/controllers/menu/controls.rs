//! The `VR: CONTROLS` page: the hand, the scheme, turning, the controller subsystems a player turns
//! on and off, their few tunings worth changing in the headset, and the saved settings (their
//! state, and a reset to defaults behind a confirmation). The rest of [`ControllersConfig`] stays
//! in the debug UI. The HUD placement settings live on `VR: DISPLAY` instead.
//!
//! [`ControllersConfig`]: crate::controllers::config::ControllersConfig

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(test))]
use parking_lot::Mutex;
use smol_str::{SmolStr, format_smolstr};

use crate::{
    config::Config,
    controllers::{
        config::{GunReadout, LocomotionDirection, SchemeSetting},
        frame::Hand,
        menu::{
            comfort, config_choice, config_slider, config_toggle, edit_config, nearest_index,
            read_config, settings_state,
        },
        vehicles::{
            self,
            config::{ArmPitch, WheelTracking},
        },
        weapons::{config::PouchSide, reload::config::ReloadButton},
    },
    headpose::config::{VrTurnConfig, VrTurnMode},
    pause_menu::model::{
        page::{Button, ButtonOutcome, Choice, Item, Page, PageDef, Readout, Toggle},
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
    on_frame: Some(on_frame),
};

fn build() -> Page {
    if CONFIRMING_RESET.load(Ordering::Relaxed) {
        return confirm_reset();
    }
    settings_page()
}

/// The page's settings, as it shows them outside the reset confirmation.
fn settings_page() -> Page {
    let page = Page::new("VR: CONTROLS")
        .keeping_focus()
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
        .item(config_toggle!("HOLSTER AND POUCH CUES", |c| c
            .controllers
            .weapons
            .haptics
            .volume_cues))
        .item(config_toggle!("ALSO RUMBLE GAMEPAD", |c| c
            .controllers
            .weapons
            .haptics
            .gamepad_rumble));
    let page = page
        .header("VEHICLES")
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
        ));
    let page = pitch_rows(page);
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

/// The reset confirmation, in place of the page's rows. Keeping the settings is the first button,
/// so it is the one focused.
fn confirm_reset() -> Page {
    Page::new("VR: CONTROLS")
        .header("RESET TO DEFAULTS")
        .text("EVERY SETTING ON THIS PAGE AND ON")
        .text("VR: COMFORT RETURNS TO ITS DEFAULT.")
        .text("THE DOMINANT HAND, THE WEAPON SIGHT")
        .text("CALIBRATION, AND THE BODY PAGE'S")
        .text("SETTINGS ARE KEPT.")
        .item(Button {
            label: "KEEP MY SETTINGS".into(),
            caption: "CANCEL".into(),
            action: || {
                CONFIRMING_RESET.store(false, Ordering::Relaxed);
                ButtonOutcome::Rebuild
            },
        })
        .item(Button {
            label: "RESET CONTROLS AND COMFORT".into(),
            caption: "RESET".into(),
            action: || {
                reset_shown_settings();
                CONFIRMING_RESET.store(false, Ordering::Relaxed);
                ButtonOutcome::Rebuild
            },
        })
}

/// Reset every setting `VR: CONTROLS` and `VR: COMFORT` show to its default, through the pages'
/// own items, so that the reset covers what the player can see and nothing else: the debug UI's
/// tuning, the body page, and the sight calibration keep their values. The dominant hand is kept
/// too: it is a choice about the player rather than a preference, and changing it takes a restart.
/// A choice the reset changes can show rows its old value hid, so the pages are reset again
/// until the rows they show stop changing.
fn reset_shown_settings() {
    let shown = || {
        let labels = |page: &Page| -> Vec<SmolStr> {
            page.items.iter().filter_map(value_label).cloned().collect()
        };
        (labels(&settings_page()), labels(&(comfort::PAGE.build)()))
    };
    let mut rows = shown();
    for _ in 0..RESET_PASSES {
        for item in &settings_page().items {
            if value_label(item).is_none_or(|label| label != DOMINANT_HAND) {
                item.reset_to_default();
            }
        }
        (comfort::PAGE.build)().reset_to_defaults();
        let now = shown();
        if now == rows {
            return;
        }
        rows = now;
    }
}

/// The label of a value item.
fn value_label(item: &Item) -> Option<&SmolStr> {
    match item {
        Item::Toggle(toggle) => Some(&toggle.label),
        Item::Choice(choice) => Some(&choice.label),
        Item::Slider(slider) => Some(&slider.label),
        Item::Header(_) | Item::Text(_) | Item::Readout(_) | Item::Button(_) => None,
    }
}

/// The most times [`reset_shown_settings`] resets the pages: once for the rows shown before, and
/// once for each level of rows that a reset choice shows.
const RESET_PASSES: usize = 4;

fn on_close() {
    CONFIRMING_RESET.store(false, Ordering::Relaxed);
}

/// Rebuild the page when the wingsuit's pitch measure changed since it was built, so that the
/// rows for the measure now chosen show.
fn on_frame() -> ButtonOutcome {
    let pitch_by = read_config(|c| c.controllers.vehicles.arm_steering.pitch_by);
    if built_pitch() == Some(pitch_by) || CONFIRMING_RESET.load(Ordering::Relaxed) {
        ButtonOutcome::Stay
    } else {
        ButtonOutcome::Rebuild
    }
}

/// The rows of the wingsuit's pitch measure: the wrists' tilt has its full tilt and a capture of
/// the relaxed tilt as the neutral; the hands' height and the arms' sweep each their full
/// distance.
fn pitch_rows(page: Page) -> Page {
    let pitch_by = read_config(|c| c.controllers.vehicles.arm_steering.pitch_by);
    set_built_pitch(pitch_by);
    match pitch_by {
        ArmPitch::WristTilt => page
            .item(config_slider!(
                "WINGSUIT FULL TILT",
                TILT_RANGE,
                SliderFormat::new(0, " DEG"),
                |c| c.controllers.vehicles.arm_steering.full_tilt_deg
            ))
            .item(Readout {
                label: "WINGSUIT NEUTRAL TILT".into(),
                read: neutral_tilt,
            })
            .item(Button {
                label: "CALIBRATE NEUTRAL".into(),
                caption: "CAPTURE".into(),
                action: capture_neutral_tilt,
            }),
        ArmPitch::Height => page.item(config_slider!(
            "WINGSUIT FULL HEIGHT",
            REACH_RANGE,
            SliderFormat::new(2, " M"),
            |c| c.controllers.vehicles.arm_steering.full_pitch_m
        )),
        ArmPitch::Sweep => page.item(config_slider!(
            "WINGSUIT FULL SWEEP",
            REACH_RANGE,
            SliderFormat::new(2, " M"),
            |c| c.controllers.vehicles.arm_steering.full_sweep_m
        )),
    }
}

/// The wingsuit's neutral wrist tilt, for a readout.
fn neutral_tilt() -> SmolStr {
    let tilt = read_config(|c| c.controllers.vehicles.arm_steering.tilt_neutral_deg);
    format_smolstr!("{tilt:.0} DEG")
}

/// Take the wrists' tilt as they are held now as the wingsuit's neutral: the tilt that pitches
/// nothing.
fn capture_neutral_tilt() -> ButtonOutcome {
    if let Some(tilt) = vehicles::wrist_tilt_now_deg() {
        edit_config(|c| {
            c.controllers.vehicles.arm_steering.tilt_neutral_deg =
                tilt.clamp(-NEUTRAL_TILT_LIMIT_DEG, NEUTRAL_TILT_LIMIT_DEG);
        });
    }
    ButtonOutcome::Stay
}

/// The furthest from flat a captured neutral tilt may be, degrees.
const NEUTRAL_TILT_LIMIT_DEG: f32 = 45.0;

/// The wingsuit pitch measure the page's rows were last built for.
#[cfg(not(test))]
static BUILT_PITCH: Mutex<Option<ArmPitch>> = Mutex::new(None);

#[cfg(not(test))]
fn built_pitch() -> Option<ArmPitch> {
    *BUILT_PITCH.lock()
}

#[cfg(not(test))]
fn set_built_pitch(pitch_by: ArmPitch) {
    *BUILT_PITCH.lock() = Some(pitch_by);
}

// Tests build pages on threads of their own, against configurations of their own.
#[cfg(test)]
thread_local! {
    static BUILT_PITCH: std::cell::Cell<Option<ArmPitch>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn built_pitch() -> Option<ArmPitch> {
    BUILT_PITCH.get()
}

#[cfg(test)]
fn set_built_pitch(pitch_by: ArmPitch) {
    BUILT_PITCH.set(Some(pitch_by));
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

/// The dominant hand's row, which the reset leaves alone.
const DOMINANT_HAND: &str = "DOMINANT HAND";

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
const POUCH_SIDE_LABELS: [&str; 2] = ["GUN HAND SIDE", "FREE HAND SIDE"];
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
/// The two-handed weight's strength, in twentieths.
const WEIGHT_RANGE: SliderRange = SliderRange::new(0.0, 2.0, 40);
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

const REACH_RANGE: SliderRange = SliderRange::new(0.1, 0.6, 50);
/// The hands' strength scale, in twentieths.
const STRENGTH_RANGE: SliderRange = SliderRange::new(0.0, 2.0, 40);

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
    use glam::{Vec2, Vec3};

    use super::*;
    use crate::{
        controllers::{
            config::SightOverride,
            menu::tests::{TEST_CONFIG, exercise_page},
        },
        grapple::GrappleComfortMode,
        pause_menu::model::rows::encode_page,
    };

    #[test]
    fn the_reset_covers_the_pages_and_keeps_the_rest() {
        let sight = SightOverride {
            weapon_hash: 7,
            offset_m: Vec3::new(0.0, 0.02, 0.0),
            angles_deg: Vec2::new(0.5, 0.0),
        };
        TEST_CONFIG.with_borrow_mut(|config| {
            *config = Config::new();
            config.controllers.aim.sights.set(sight);
            config.controllers.dominant_hand = Hand::Left;
            config.controllers.scheme = SchemeSetting::Gamepad;
            config.controllers.locomotion.stick.acceleration_mps2 = 3.0;
            config.headpose.vr_turn.snap_angle_deg = 45.0;
            config.headpose.grapple.mode = GrappleComfortMode::Off;
            // Not on either page: the debug UI's tuning, the body page, and the display.
            config.controllers.hands.reach.pull_max = 0.5;
            config.controllers.body.procedural.gait = false;
            config.headpose.mouse_sensitivity = 3.0;
            config.hud.distance = 2.0;
        });
        reset_shown_settings();
        let defaults = Config::new();
        read_config(|config| {
            assert_eq!(config.controllers.scheme, defaults.controllers.scheme);
            assert_eq!(
                config.controllers.locomotion.stick.acceleration_mps2,
                defaults.controllers.locomotion.stick.acceleration_mps2
            );
            assert_eq!(
                config.headpose.vr_turn.snap_angle_deg,
                defaults.headpose.vr_turn.snap_angle_deg
            );
            assert_eq!(config.headpose.grapple.mode, defaults.headpose.grapple.mode);

            assert_eq!(config.controllers.dominant_hand, Hand::Left);
            assert_eq!(config.controllers.aim.sights.get(7), Some(&sight));
            assert_eq!(config.controllers.hands.reach.pull_max, 0.5);
            assert!(!config.controllers.body.procedural.gait);
            assert_eq!(config.headpose.mouse_sensitivity, 3.0);
            assert_eq!(config.hud.distance, 2.0);
        });
    }

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
    fn the_wingsuit_rows_follow_the_pitch_measure() {
        let labels = |pitch_by| {
            edit_config(|c| c.controllers.vehicles.arm_steering.pitch_by = pitch_by);
            settings_page()
                .items
                .iter()
                .filter_map(value_label)
                .filter(|label| label.starts_with("WINGSUIT FULL"))
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(ArmPitch::WristTilt), ["WINGSUIT FULL TILT"]);
        assert_eq!(labels(ArmPitch::Height), ["WINGSUIT FULL HEIGHT"]);
        assert_eq!(labels(ArmPitch::Sweep), ["WINGSUIT FULL SWEEP"]);
        let calibrate = |page: &Page| {
            page.items
                .iter()
                .any(|item| matches!(item, Item::Button(b) if b.label == "CALIBRATE NEUTRAL"))
        };
        assert!(!calibrate(&settings_page()));
        edit_config(|c| c.controllers.vehicles.arm_steering.pitch_by = ArmPitch::WristTilt);
        assert!(calibrate(&settings_page()));
        // A change of measure since the build asks for a rebuild, and none is asked after it.
        assert_eq!(on_frame(), ButtonOutcome::Stay);
        edit_config(|c| c.controllers.vehicles.arm_steering.pitch_by = ArmPitch::Height);
        assert_eq!(on_frame(), ButtonOutcome::Rebuild);
        settings_page();
        assert_eq!(on_frame(), ButtonOutcome::Stay);
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
