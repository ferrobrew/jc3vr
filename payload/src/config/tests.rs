//! Tests for the settings file: the round trip through TOML and the tolerant load.

use std::borrow::Cow;

use glam::{Vec2, Vec3};
use pretty_assertions::assert_eq;
use toml::{Table, Value};

use super::*;
use crate::{
    config::load::{LoadReport, Loaded, parse},
    controllers::{
        config::{Posture, SchemeSetting, SightOverride},
        frame::Hand,
    },
    director::keymap::{Action, Binding, Chord},
};

#[test]
fn the_defaults_round_trip() {
    let text = toml::to_string(&Config::new()).unwrap();
    let Loaded { config, report } = parse(&text).unwrap();
    assert_eq!(report, LoadReport::default());
    assert_eq!(toml::to_string(&config).unwrap(), text);
}

#[test]
fn changed_settings_round_trip() {
    let mut original = Config::new();
    // Options whose default is `None`, set: the defaults leave the key out, the file has it.
    original.hud.render_resolution = Some(2048);
    original.vr.loader_path = Some("C:/openxr/openxr_loader.dll".to_owned());
    original.director.heat_cap = Some(3);
    original.body.seated_eye_height = Some(1.2);
    original.body.height = 1.9;
    // A list that is not the default's length, and lists of tables.
    original.director.keymap.bindings = Cow::Owned(vec![Binding {
        action: Action::TakeMarker,
        chord: Chord {
            key: 0x4D,
            ctrl: true,
            shift: false,
            alt: false,
        },
    }]);
    original.controllers.aim.sights.set(SightOverride {
        weapon_hash: 42,
        offset_m: Vec3::new(0.0, 0.03, 0.0),
        angles_deg: Vec2::new(1.0, -2.0),
    });
    original.controllers.fingers.curl_ranges[2].closed = 0.8;
    // An enum, a borrowed default string made owned, and a switch beside its value.
    original.controllers.dominant_hand = Hand::Left;
    original.far_field.gated_types = Cow::Owned("Window".to_owned());
    original.fsr.sharpen = false;
    // Values JSON could not carry, in settings no validation covers.
    original.vr.far_clip = f32::INFINITY;
    original.exposure.forced_value = f32::NAN;

    let text = toml::to_string(&original).unwrap();
    let Loaded { config, report } = parse(&text).unwrap();
    assert_eq!(report, LoadReport::default());
    assert_eq!(toml::to_string(&config).unwrap(), text);
    assert_eq!(config.director.keymap.bindings.len(), 1);
    assert_eq!(config.body.seated_eye_height, Some(1.2));
    assert!(config.exposure.forced_value.is_nan());
}

#[test]
fn a_missing_optional_setting_loads_as_none() {
    let mut table = defaults_table();
    assert!(lookup(&table, &["body", "seated_eye_height"]).is_none());
    remove(&mut table, &["director", "heat_cap"]);
    let Loaded { config, report } = load(table);
    assert_eq!(config.body.seated_eye_height, None);
    assert_eq!(config.director.heat_cap, None);
    assert!(report.defaulted.is_empty(), "{report:?}");
}

#[test]
fn a_missing_setting_takes_its_default() {
    let mut table = defaults_table();
    remove(&mut table, &["stereo", "ipd"]);
    remove(&mut table, &["recorder"]);
    set(&mut table, &["controllers", "dominant_hand"], "Left".into());
    let Loaded { config, report } = load(table);
    assert_eq!(config.stereo.ipd, Config::new().stereo.ipd);
    assert_eq!(config.recorder.port, Config::new().recorder.port);
    assert_eq!(config.controllers.dominant_hand, Hand::Left);
    assert_eq!(report.defaulted, ["recorder", "stereo.ipd"]);
    assert!(report.ignored.is_empty() && report.reset.is_empty());
}

#[test]
fn a_saved_off_wingsuit_readout_takes_the_new_default() {
    // A file saved while the readout was off by default holds the earlier key, which no longer
    // loads, so the panel comes on.
    let mut table = defaults_table();
    let arm_steering = ["controllers", "vehicles", "arm_steering"];
    remove(
        &mut table,
        &[&arm_steering[..], &["show_readout_panel"]].concat(),
    );
    set(
        &mut table,
        &[&arm_steering[..], &["show_readout"]].concat(),
        false.into(),
    );
    let Loaded { config, report } = load(table);
    assert!(config.controllers.vehicles.arm_steering.show_readout_panel);
    assert_eq!(
        report.ignored,
        ["controllers.vehicles.arm_steering.show_readout"]
    );
}

#[test]
fn an_unknown_setting_is_skipped_and_reported() {
    let mut table = defaults_table();
    set(&mut table, &["stereo", "retired_setting"], 0.5.into());
    set(&mut table, &["obsolete"], true.into());
    // A setting that is not saved, written by hand, is skipped like an unknown one.
    set(&mut table, &["director", "paused"], true.into());
    let Loaded { config, report } = load(table);
    assert!(!config.director.paused);
    assert_eq!(
        report.ignored,
        ["director.paused", "obsolete", "stereo.retired_setting"]
    );
    assert!(report.defaulted.is_empty() && report.reset.is_empty());
}

#[test]
fn a_file_from_before_the_rig_loads_its_posture_and_skips_the_retired_settings() {
    let mut table = defaults_table();
    set(
        &mut table,
        &["controllers", "roomscale", "posture"],
        "Roomscale".into(),
    );
    set(
        &mut table,
        &["controllers", "roomscale", "roomscale_min_extent_m"],
        1.0.into(),
    );
    set(
        &mut table,
        &["controllers", "roomscale", "chase", "deadzone_m"],
        0.1.into(),
    );
    set(
        &mut table,
        &["controllers", "body", "alignment", "root_follow"],
        true.into(),
    );
    set(
        &mut table,
        &["controllers", "roomscale", "stance", "square"],
        true.into(),
    );
    set(
        &mut table,
        &["controllers", "body", "procedural", "step_under_head"],
        true.into(),
    );
    set(
        &mut table,
        &["controllers", "roomscale", "crouch", "visual"],
        true.into(),
    );
    let Loaded { config, report } = load(table);
    assert_eq!(config.controllers.roomscale.posture, Posture::Standing);
    assert!(config.controllers.body.rig.chase.enabled);
    assert_eq!(
        report.ignored,
        [
            "controllers.body.alignment",
            "controllers.body.procedural.step_under_head",
            "controllers.roomscale.chase",
            "controllers.roomscale.crouch.visual",
            "controllers.roomscale.roomscale_min_extent_m",
            "controllers.roomscale.stance",
        ]
    );
    assert!(report.reset.is_empty());
}

#[test]
fn a_mistyped_setting_is_reset_alone() {
    let mut table = defaults_table();
    set(
        &mut table,
        &["controllers", "hands", "reach_t"],
        "far".into(),
    );
    set(&mut table, &["stereo", "ipd"], 0.07.into());
    set(&mut table, &["controllers", "dominant_hand"], "Left".into());
    let Loaded { config, report } = load(table);
    assert_eq!(
        config.controllers.hands.reach_t,
        Config::new().controllers.hands.reach_t
    );
    assert_eq!(config.stereo.ipd, 0.07);
    assert_eq!(config.controllers.dominant_hand, Hand::Left);
    assert_eq!(reset_paths(&report), ["controllers.hands.reach_t"]);
}

#[test]
fn an_unknown_enum_variant_is_reset_alone() {
    let mut table = defaults_table();
    set(&mut table, &["controllers", "scheme"], "Telepathy".into());
    set(&mut table, &["controllers", "dominant_hand"], "Left".into());
    let Loaded { config, report } = load(table);
    assert_eq!(config.controllers.scheme, SchemeSetting::Controllers);
    assert_eq!(config.controllers.dominant_hand, Hand::Left);
    assert_eq!(reset_paths(&report), ["controllers.scheme"]);
}

#[test]
fn a_list_of_the_wrong_length_is_reset_alone() {
    let mut table = defaults_table();
    set(
        &mut table,
        &["controllers", "fingers", "finger_bend_deg"],
        Value::Array(vec![1.0.into()]),
    );
    set(
        &mut table,
        &["controllers", "fingers", "splay_scale"],
        2.0.into(),
    );
    let Loaded { config, report } = load(table);
    assert_eq!(
        config.controllers.fingers.finger_bend_deg,
        Config::new().controllers.fingers.finger_bend_deg
    );
    assert_eq!(config.controllers.fingers.splay_scale, 2.0);
    assert_eq!(
        reset_paths(&report),
        ["controllers.fingers.finger_bend_deg"]
    );
}

#[test]
fn several_bad_settings_are_each_reset() {
    let mut table = defaults_table();
    set(&mut table, &["controllers", "scheme"], 3.into());
    set(&mut table, &["vr", "mirror"], "yes".into());
    set(&mut table, &["hud", "render_resolution"], "high".into());
    let Loaded { config, report } = load(table);
    assert!(config.vr.mirror);
    assert_eq!(config.hud.render_resolution, None);
    let mut paths = reset_paths(&report);
    paths.sort();
    assert_eq!(
        paths,
        ["controllers.scheme", "hud.render_resolution", "vr.mirror"]
    );
}

#[test]
fn an_invalid_controller_setting_is_reset_alone() {
    let mut config = Config::new();
    config.controllers.dominant_hand = Hand::Left;
    config.controllers.aim.sights.set(SightOverride {
        weapon_hash: 7,
        offset_m: Vec3::new(0.0, 0.01, 0.0),
        angles_deg: Vec2::new(0.5, 0.0),
    });
    config.controllers.hands.reach_t = 5.0;
    config.controllers.hands.reach_r = 0.25;
    config.headpose.vr_turn.snap_angle_deg = 45.0;
    let Loaded { config, report } = parse(&toml::to_string(&config).unwrap()).unwrap();
    assert_eq!(config.controllers.dominant_hand, Hand::Left);
    assert_eq!(
        config.controllers.hands.reach_t,
        Config::new().controllers.hands.reach_t
    );
    assert_eq!(config.controllers.hands.reach_r, 0.25);
    assert!(config.controllers.aim.sights.get(7).is_some());
    assert_eq!(config.headpose.vr_turn.snap_angle_deg, 45.0);
    assert_eq!(reset_paths(&report), ["controllers.hands.reach_t"]);
}

#[test]
fn an_invalid_vector_component_resets_the_vector() {
    let mut config = Config::new();
    config.controllers.hands.grip.position_m = Vec3::new(0.0, 99.0, 0.0);
    config.controllers.hands.grip.rotation_deg = Vec3::new(5.0, 0.0, 0.0);
    let Loaded { config, report } = parse(&toml::to_string(&config).unwrap()).unwrap();
    assert_eq!(
        config.controllers.hands.grip.position_m,
        Config::new().controllers.hands.grip.position_m
    );
    assert_eq!(
        config.controllers.hands.grip.rotation_deg,
        Vec3::new(5.0, 0.0, 0.0)
    );
    assert_eq!(reset_paths(&report), ["controllers.hands.grip.position_m"]);
}

#[test]
fn an_inconsistency_the_default_does_not_fix_resets_the_group() {
    let mut config = Config::new();
    config.controllers.dominant_hand = Hand::Left;
    // The far reach keeps its default, which is below this near reach: resetting the setting the
    // check names does not help, so the group around it is reset.
    config.controllers.weapons.heft.reach_near_m = 9.0;
    let Loaded { config, report } = parse(&toml::to_string(&config).unwrap()).unwrap();
    assert_eq!(
        config.controllers.weapons.heft.reach_near_m,
        Config::new().controllers.weapons.heft.reach_near_m
    );
    assert_eq!(config.controllers.dominant_hand, Hand::Left);
    assert_eq!(reset_paths(&report), ["controllers.weapons.heft"]);
}

#[test]
fn several_invalid_controller_settings_are_each_reset() {
    let mut config = Config::new();
    config.controllers.hands.reach_t = 5.0;
    config.controllers.bindings.move_deadzone = -1.0;
    let Loaded { config, report } = parse(&toml::to_string(&config).unwrap()).unwrap();
    assert_eq!(
        config.controllers.bindings.move_deadzone,
        Config::new().controllers.bindings.move_deadzone
    );
    assert_eq!(
        reset_paths(&report),
        [
            "controllers.bindings.move_deadzone",
            "controllers.hands.reach_t"
        ]
    );
}

#[test]
fn invalid_sights_turn_and_body_reset_alone() {
    let mut config = Config::new();
    let sight = SightOverride {
        weapon_hash: 7,
        offset_m: Vec3::ZERO,
        angles_deg: Vec2::ZERO,
    };
    config.controllers.aim.sights.entries[0] = Some(sight);
    config.controllers.aim.sights.entries[1] = Some(sight);
    config.controllers.dominant_hand = Hand::Left;
    config.headpose.vr_turn.deadzone = 2.0;
    config.body.eye_height = 2.5;
    let Loaded { config, report } = parse(&toml::to_string(&config).unwrap()).unwrap();
    assert!(config.controllers.aim.sights.get(7).is_none());
    assert_eq!(config.controllers.dominant_hand, Hand::Left);
    assert_eq!(
        config.headpose.vr_turn.deadzone,
        Config::new().headpose.vr_turn.deadzone
    );
    assert_eq!(config.body, Config::new().body);
    assert_eq!(
        reset_paths(&report),
        [
            "controllers.aim.sights",
            "headpose.vr_turn.deadzone",
            "body"
        ]
    );
}

#[test]
fn a_shorter_key_map_loads_as_written() {
    let mut table = defaults_table();
    set(
        &mut table,
        &["director", "keymap", "bindings"],
        Value::Array(vec![Value::Table(
            toml::from_str("action = \"Spawn\"\n[chord]\nkey = 0x61\n").unwrap(),
        )]),
    );
    let Loaded { config, report } = load(table);
    assert_eq!(
        config.director.keymap.bindings.as_ref(),
        [Binding {
            action: Action::Spawn,
            chord: Chord {
                key: 0x61,
                ctrl: false,
                shift: false,
                alt: false,
            },
        }]
    );
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn text_that_is_not_toml_is_an_error() {
    assert!(matches!(
        parse("stereo = {"),
        Err(load::LoadError::Syntax(_))
    ));
}

#[test]
fn an_automatic_switch_off_keeps_the_saved_choice_until_the_user_changes_it() {
    let switch = Switch {
        path: "vr.mirror",
        place: |c| &mut c.vr.mirror,
    };
    let mut config = Config::new();
    automatic::disable(&mut config, switch);
    assert!(!config.vr.mirror);
    let mut snapshot = config.clone();
    automatic::apply(&mut snapshot);
    assert!(snapshot.vr.mirror);

    // An edit that leaves the switch alone keeps the override.
    let before = automatic::observe(&mut config);
    config.stereo.ipd = 0.07;
    automatic::settle(&mut config, &before);
    let mut snapshot = config.clone();
    automatic::apply(&mut snapshot);
    assert!(snapshot.vr.mirror);

    // An edit that changes it makes the live value the user's.
    let before = automatic::observe(&mut config);
    config.vr.mirror = true;
    automatic::settle(&mut config, &before);
    config.vr.mirror = false;
    let mut snapshot = config.clone();
    automatic::apply(&mut snapshot);
    assert!(!snapshot.vr.mirror);
}

fn defaults_table() -> Table {
    toml::from_str(&toml::to_string(&Config::new()).unwrap()).unwrap()
}

fn load(table: Table) -> Loaded {
    parse(&toml::to_string(&table).unwrap()).unwrap()
}

fn reset_paths(report: &LoadReport) -> Vec<&str> {
    report.reset.iter().map(|r| r.path.as_str()).collect()
}

fn lookup<'a>(table: &'a Table, path: &[&str]) -> Option<&'a Value> {
    let (last, parents) = path.split_last()?;
    let mut table = table;
    for key in parents {
        table = table.get(*key)?.as_table()?;
    }
    table.get(*last)
}

fn parent_mut<'a>(table: &'a mut Table, parents: &[&str]) -> &'a mut Table {
    parents.iter().fold(table, |table, key| {
        table
            .entry(*key)
            .or_insert_with(|| Value::Table(Table::new()))
            .as_table_mut()
            .unwrap()
    })
}

fn set(table: &mut Table, path: &[&str], value: Value) {
    let (last, parents) = path.split_last().unwrap();
    parent_mut(table, parents).insert((*last).to_owned(), value);
}

fn remove(table: &mut Table, path: &[&str]) {
    let (last, parents) = path.split_last().unwrap();
    parent_mut(table, parents).remove(*last);
}
