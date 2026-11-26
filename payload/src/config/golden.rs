//! Golden tests for the settings file, recorded from the configuration module before its engine
//! was separated from the aggregate, so that the file a save writes and what a load makes of one
//! stay as they were; and tests of the store as the payload uses it.

use std::{
    borrow::Cow,
    panic::{self, AssertUnwindSafe},
};

use glam::{Vec2, Vec3};
use jc3vr_core::config::engine::persist::edit_pending;
use jc3vr_gameplay::controllers::config::SightOverride;
use jc3vr_hud::hud::config::HUD_CONFIG;
use jc3vr_input::frame::Hand;
use jc3vr_render::vr::{MirrorView, VR_CONFIG};
use jc3vr_tooling::director::keymap::{Action, Binding, Chord};
use pretty_assertions::assert_eq;

use crate::config::{CONFIG, Config, edit, format, load::parse, test_fixture};

/// A configuration with settings changed across many groups and value kinds: floats that widen
/// from `f32`, an option, a list of another length, a list of tables, an enum, a borrowed string
/// made owned, and a non-finite value.
fn non_default() -> Config {
    let mut config = Config::new();
    config.vr.loader_path = Some("C:/openxr/openxr_loader.dll".to_owned());
    config.director.heat_cap = Some(3);
    config.body.seated_eye_height = Some(1.21);
    config.body.height = 1.849;
    config.director.keymap.bindings = Cow::Owned(vec![Binding {
        action: Action::TakeMarker,
        chord: Chord {
            key: 0x4D,
            ctrl: true,
            shift: false,
            alt: false,
        },
    }]);
    config.controllers.aim.sights.set(SightOverride {
        weapon_hash: 42,
        offset_m: Vec3::new(0.0, 0.03, 0.0),
        angles_deg: Vec2::new(1.0, -2.0),
    });
    config.controllers.hands.grip.position_m = Vec3::new(0.01, 0.07, 0.0);
    config.controllers.dominant_hand = Hand::Left;
    config.controllers.body.rig.yaw.moving_deadzone_deg = 30.0;
    config.far_field.gated_types = Cow::Owned("Window".to_owned());
    config.fsr.sharpen = true;
    config.fsr.sharpness = 0.07;
    config.hud.distance = 1.37;
    config.vr.mirror_view = MirrorView::StereoPair;
    config.vr.far_clip = f32::INFINITY;
    config
}

const NON_DEFAULT_TEXT: &str = r#"version = 1

[body]
height = 1.849
seated_eye_height = 1.21

[controllers]
dominant_hand = "Left"

[[controllers.aim.sights]]
angles_deg = [1.0, -2.0]
offset_m = [0.0, 0.03, 0.0]
weapon_hash = 42

[controllers.body.rig.yaw]
moving_deadzone_deg = 30.0

[controllers.hands.grip]
position_m = [0.01, 0.07, 0.0]

[director]
heat_cap = 3

[[director.keymap.bindings]]
action = "TakeMarker"

[director.keymap.bindings.chord]
alt = false
ctrl = true
key = 77
shift = false

[far_field]
gated_types = "Window"

[fsr]
sharpen = true
sharpness = 0.07

[hud]
distance = 1.37

[vr]
far_clip = inf
loader_path = "C:/openxr/openxr_loader.dll"
mirror_view = "StereoPair"
"#;

/// A file with a bad value of every kind the load handles: an unknown setting, a value of the
/// wrong type, an unknown enum variant, a list of the wrong length, values that fail validation,
/// and a migrated switch.
const MESSY_TEXT: &str = r#"
version = 1
obsolete = true
[controllers.aim]
assist_strength = 5.0
[controllers.locomotion]
direction = "Telepathy"
[controllers.hands.grip]
position_m = [0.01]
[hud]
distance = "far"
[headpose.vr_turn]
deadzone = 2.0
[body]
eye_height = 2.5
[fsr]
enabled = false
"#;

const MESSY_REPORT: &str = r#"LoadReport {
    defaulted: [],
    migrated: [
        Migrated {
            path: "fsr.enabled",
            removed: "false",
        },
    ],
    newer_version: None,
    ignored: [
        "obsolete",
    ],
    reset: [
        Reset {
            path: "controllers.hands.grip.position_m",
            reason: "invalid length 1, expected a sequence of 3 f32 values",
        },
        Reset {
            path: "controllers.locomotion.direction",
            reason: "unknown variant `Telepathy`, expected `Head` or `OffHand`",
        },
        Reset {
            path: "hud.distance",
            reason: "invalid type: string \"far\", expected f32",
        },
        Reset {
            path: "controllers.aim.assist_strength",
            reason: "invalid setting: aim.assist_strength must be finite and in [0, 1], got 5",
        },
        Reset {
            path: "headpose.vr_turn.deadzone",
            reason: "invalid setting: deadzone must be finite and in [0, 1], got 2",
        },
        Reset {
            path: "body",
            reason: "invalid setting: `eye_height` must be below `height`",
        },
    ],
}"#;

#[test]
fn a_non_default_configuration_saves_as_it_did() {
    assert_eq!(format::to_text(&non_default()).unwrap(), NON_DEFAULT_TEXT);
}

#[test]
fn the_saved_text_loads_back_to_the_same_configuration() {
    let loaded = parse(NON_DEFAULT_TEXT).unwrap();
    assert!(loaded.report.is_empty(), "{:?}", loaded.report);
    assert_eq!(
        toml::to_string(&loaded.config).unwrap(),
        toml::to_string(&non_default()).unwrap()
    );
    assert_eq!(format::to_text(&loaded.config).unwrap(), NON_DEFAULT_TEXT);
}

#[test]
fn a_messy_file_loads_as_it_did() {
    let loaded = parse(MESSY_TEXT).unwrap();
    assert_eq!(format!("{:#?}", loaded.report), MESSY_REPORT);
    assert_eq!(
        format::to_text(&loaded.config).unwrap(),
        "version = 1\n\n[fsr]\nantialiasing = \"Engine\"\n"
    );
}

#[test]
fn the_fixture_restores_both_the_settings_and_the_edit_status() {
    {
        let _fixture = test_fixture();
        edit().hud.distance = 42.0;
        assert!(edit_pending());
    }
    let _fixture = test_fixture();
    assert_eq!(CONFIG.lock().hud.distance, Config::new().hud.distance);
    assert!(!edit_pending());
}

#[test]
fn a_slot_reads_and_writes_its_group_of_the_one_configuration() {
    let _fixture = test_fixture();
    HUD_CONFIG.lock().distance = 2.5;
    assert_eq!(CONFIG.lock().hud.distance, 2.5);
    CONFIG.lock().vr.mirror = false;
    assert!(!VR_CONFIG.lock().mirror);
    VR_CONFIG.edit().mirror = true;
    assert!(CONFIG.lock().vr.mirror);
}

#[cfg(debug_assertions)]
#[test]
fn a_second_lock_on_one_thread_panics_rather_than_deadlocking() {
    let _fixture = test_fixture();
    let guard = HUD_CONFIG.lock();
    let nested = panic::catch_unwind(AssertUnwindSafe(|| VR_CONFIG.lock().mirror));
    assert!(nested.is_err());
    let nested = panic::catch_unwind(AssertUnwindSafe(|| CONFIG.lock().vr.mirror));
    assert!(nested.is_err());
    drop(guard);
    // The guard's release clears the mark, so the next lock succeeds.
    let _ = VR_CONFIG.lock().mirror;
}
