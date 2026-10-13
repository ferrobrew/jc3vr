//! The `VR: CONTROLS` page: the hand, the scheme, turning, the controller subsystems a player turns
//! on and off, their few tunings worth changing in the headset, and the saved settings (their
//! state, and a reset to defaults behind a confirmation). The rest of [`ControllersConfig`] stays
//! in the debug UI. The HUD placement settings live on `VR: DISPLAY` instead.
//!
//! [`ControllersConfig`]: crate::controllers::config::ControllersConfig

mod sections;

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(test))]
use parking_lot::Mutex;
use sections::DOMINANT_HAND;
use smol_str::{SmolStr, format_smolstr};

use crate::{
    controllers::{
        menu::{comfort, config_slider, edit_config, read_config},
        vehicles::{self, config::ArmPitch},
    },
    pause_menu::model::{
        page::{Button, ButtonOutcome, Item, Page, PageDef, Readout},
        slider::{SliderFormat, SliderRange},
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

/// The page's settings, as it shows them outside the reset confirmation, assembled section by
/// section ([`sections`]).
fn settings_page() -> Page {
    let page = Page::new("VR: CONTROLS").keeping_focus();
    let page = sections::controllers_section(page);
    let page = sections::turning_section(page);
    let page = sections::hands_and_aim_section(page);
    let page = sections::grapple_section(page);
    let page = sections::controls_summary_section(page);
    let page = sections::weapons_section(page);
    let page = sections::haptics_section(page);
    let page = sections::vehicles_section(page);
    let page = pitch_rows(page);
    sections::parachute_and_saved_settings_section(page)
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

/// Whether the page shows the reset confirmation.
static CONFIRMING_RESET: AtomicBool = AtomicBool::new(false);

/// The wingsuit's full pitch by wrist tilt, degrees, in degrees.
const TILT_RANGE: SliderRange = SliderRange::new(10.0, 60.0, 50);

const REACH_RANGE: SliderRange = SliderRange::new(0.1, 0.6, 50);

#[cfg(test)]
mod tests {
    use glam::{Vec2, Vec3};

    use super::{
        sections::{
            ARM_PITCH_LABELS, DIRECTION_LABELS, DIRECTIONS, GUN_READOUT_LABELS, GUN_READOUTS,
            HAND_LABELS, HANDS, SCHEME_LABELS, SCHEMES, SNAP_ANGLE_LABELS, SNAP_ANGLES,
            TURN_MODE_LABELS, TURN_MODES, WHEEL_TRACKING, WHEEL_TRACKING_LABELS, layout_label,
        },
        *,
    };
    use crate::{
        config::Config,
        controllers::{
            config::{SchemeSetting, SightOverride},
            frame::Hand,
            menu::{
                nearest_index,
                tests::{TEST_CONFIG, exercise_page},
            },
        },
        grapple::GrappleComfortMode,
        headpose::config::VrTurnConfig,
        pause_menu::model::rows::encode_page,
        vr::input::{InputStatus, restart::RestartReason},
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
            config.controllers.body.rig.legs.gait = false;
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
            assert!(!config.controllers.body.rig.legs.gait);
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
