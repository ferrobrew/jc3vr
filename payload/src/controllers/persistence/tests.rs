//! Tests for the settings file's format, merge, validation, and location.

use glam::{Vec2, Vec3};
use serde_json::json;

use super::*;
use crate::controllers::{config::SightOverride, frame::Hand};

#[test]
fn the_versioned_format_round_trips_a_real_controller_file() {
    let mut original = Config::new();
    original.controllers.dominant_hand = Hand::Left;
    original.controllers.hands.grip.position_m = Vec3::new(0.1, -0.2, 0.3);
    original.controllers.aim.sights.set(SightOverride {
        weapon_hash: 42,
        offset_m: Vec3::new(0.0, 0.03, 0.0),
        angles_deg: Vec2::new(1.0, -2.0),
    });
    original.headpose.vr_turn.smooth_scale = 1.75;
    let text = serde_json::to_string(&from_config(&original)).unwrap();
    let parsed = parse(&text).unwrap();
    assert!(parsed.report.is_empty(), "{:?}", parsed.report);
    let mut loaded = Config::new();
    apply(&parsed.file, &mut loaded).unwrap();
    assert_eq!(loaded.controllers.dominant_hand, Hand::Left);
    assert_eq!(
        loaded.controllers.hands.grip.position_m,
        Vec3::new(0.1, -0.2, 0.3)
    );
    assert_eq!(
        loaded.controllers.aim.sights.get(42).unwrap().angles_deg,
        Vec2::new(1.0, -2.0)
    );
    assert_eq!(loaded.headpose.vr_turn.smooth_scale, 1.75);
}

#[test]
fn a_partial_file_loads_with_the_missing_settings_defaulted() {
    let text = json!({
        "version": FILE_VERSION,
        "controllers": {
            "dominant_hand": "Left",
            "hands": { "grip": { "position_m": [0.1, -0.2, 0.3] } },
            "aim": {
                "sights": { "entries": [
                    { "weapon_hash": 7, "offset_m": [0.0, 0.01, 0.0], "angles_deg": [0.5, 0.0] }
                ] }
            }
        }
    })
    .to_string();
    let parsed = parse(&text).unwrap();
    let defaults = ControllersConfig::new();
    let c = &parsed.file.controllers;
    assert_eq!(c.dominant_hand, Hand::Left);
    assert_eq!(c.hands.grip.position_m, Vec3::new(0.1, -0.2, 0.3));
    assert_eq!(c.hands.grip.rotation_deg, defaults.hands.grip.rotation_deg);
    assert_eq!(c.hands.reach_t, defaults.hands.reach_t);
    assert_eq!(
        c.aim.sights.get(7).unwrap().offset_m,
        Vec3::new(0.0, 0.01, 0.0)
    );
    assert_eq!(c.aim.max_distance_m, defaults.aim.max_distance_m);
    assert_eq!(
        parsed.file.headpose.vr_turn.smooth_scale,
        VrTurnConfig::new().smooth_scale
    );
    let defaulted = &parsed.report.defaulted;
    assert!(defaulted.contains(&"headpose".to_owned()));
    assert!(defaulted.contains(&"controllers.hands.grip.rotation_deg".to_owned()));
    assert!(defaulted.contains(&"controllers.aim.sights.entries[1]".to_owned()));
    assert!(!defaulted.contains(&"controllers.hands.grip.position_m".to_owned()));
    assert!(parsed.report.ignored.is_empty());
}

#[test]
fn unknown_settings_are_ignored_and_reported() {
    let mut value = serde_json::to_value(from_config(&Config::new())).unwrap();
    value["controllers"]["hands"]["retired_setting"] = 0.1.into();
    value["controllers"]["hands"]["grip"]["position_m"] = json!([0.0, 0.0, 0.0, 9.0]);
    value["obsolete"] = true.into();
    let parsed = parse(&value.to_string()).unwrap();
    assert_eq!(
        parsed.report.ignored,
        [
            "controllers.hands.grip.position_m[3]",
            "controllers.hands.retired_setting",
            "obsolete",
        ]
    );
    assert!(parsed.report.defaulted.is_empty());
    assert_eq!(parsed.file.controllers.hands.grip.position_m, Vec3::ZERO);
}

#[test]
fn a_mistyped_setting_is_rejected() {
    let mut value = serde_json::to_value(from_config(&Config::new())).unwrap();
    value["controllers"]["hands"]["reach_t"] = "far".into();
    assert!(matches!(
        parse(&value.to_string()),
        Err(Error::Deserialize(_))
    ));
}

#[test]
fn unknown_and_missing_versions_are_rejected_before_deserialization() {
    let text = r#"{"version":99,"controllers":{},"headpose":{}}"#;
    assert!(matches!(parse(text), Err(Error::UnknownVersion(99))));
    let text = r#"{"controllers":{}}"#;
    assert!(matches!(parse(text), Err(Error::MissingVersion)));
    assert!(matches!(parse("{"), Err(Error::Parse(_))));
}

#[test]
fn an_invalid_file_does_not_partially_apply() {
    let mut loaded = Config::new();
    let before = loaded.controllers.hands.grip.position_m;
    let mut file = from_config(&loaded);
    file.controllers.hands.grip.position_m.x = 0.25;
    file.controllers.aim.grapple_origin_m.x = f32::NAN;
    assert!(apply(&file, &mut loaded).is_err());
    assert_eq!(loaded.controllers.hands.grip.position_m, before);
}

#[test]
fn zero_cooldown_and_instant_blends_are_valid() {
    let mut file = from_config(&Config::new());
    file.controllers.bindings.yank.cooldown_s = 0.0;
    file.controllers.hands.yield_blend_s = 0.0;
    file.controllers.hands.return_blend_s = 0.0;
    file.controllers.hands.anchor.blend_s = 0.0;
    file.controllers.weapons.foregrip.blend_s = 0.0;
    file.controllers.weapons.foregrip.min_span_m = 0.0;
    file.controllers.weapons.grenade.release_peak_span_s = 0.0;
    file.controllers.roomscale.fade.out_s = 0.0;
    file.controllers.roomscale.fade.in_s = 0.0;
    file.controllers.vehicles.physical.wheel.return_deg_per_s = 0.0;
    file.controllers.vehicles.physical.wheel.lock_deg = 540.0;
    validate::validate(&file).unwrap();
    parse(&serde_json::to_string(&file).unwrap()).unwrap();
}

#[test]
fn nonfinite_nested_tuning_is_rejected_before_mutation() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut config = Config::new();
        let mut file = from_config(&config);
        file.controllers.fingers.finger_bend_deg[0] = value;
        assert!(apply(&file, &mut config).is_err());
        file = from_config(&config);
        file.controllers.weapons.grenade.warning_interval_s[0] = value;
        assert!(apply(&file, &mut config).is_err());
    }
}

#[test]
fn the_settings_live_above_the_build_output() {
    assert_eq!(
        settings_dir(Path::new(
            "C:/work/jc3vr/target/x86_64-pc-windows-msvc/release"
        )),
        Path::new("C:/work/jc3vr")
    );
    assert_eq!(
        settings_dir(Path::new("C:/work/jc3vr/Target/debug")),
        Path::new("C:/work/jc3vr")
    );
    assert_eq!(
        settings_dir(Path::new("C:/games/jc3vr")),
        Path::new("C:/games/jc3vr")
    );
}
