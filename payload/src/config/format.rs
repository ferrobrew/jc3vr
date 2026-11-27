//! The settings file's format for this configuration: the engine's diff-from-defaults text
//! ([`jc3vr_core::config::format`]) for a [`Config`], and the migrations of a file an older
//! build wrote, which name the subsystems' settings.
//!
//! Format zero, a file without a version, holds every setting, each default as it stood when the
//! file was written, so a default changed since is indistinguishable from the player's choice.
//! [`migrate`] removes each numeric tuning of such a file that holds a default a later build
//! replaced ([`SUPERSEDED`]), so that it takes the current default, as it would have had the file
//! been written in the current format. A switch or a mode is kept as written, since a player who
//! set it back to its old default cannot be told from one who never touched it. The one exception
//! is the single-pass stereo group, which a file of format zero holds turned off as it shipped
//! then: [`migrate`] removes the group's switches that are off, so the group takes its current
//! default as a whole rather than loading half on.

#[cfg(test)]
use jc3vr_core::config as engine;
#[cfg(test)]
use jc3vr_core::config::format::TextError;
use jc3vr_core::config::format::{Format, Migrated, remove_superseded, same};
use jc3vr_tooling::director::keymap::{Action, Chord, Keymap};
use toml::{Table, Value};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_NUMPAD0;

#[cfg(test)]
use crate::config::Config;
#[cfg(test)]
use crate::config::schema::SCHEMA;

/// The configuration as a save writes it: the version, then each setting that differs from its
/// default.
#[cfg(test)]
pub(crate) fn to_text(config: &Config) -> Result<String, TextError> {
    engine::format::to_text(&SCHEMA, config)
}

/// The migrations of a settings file's table, its version already taken out, written in `format`.
/// Every file loses the settings a later build renamed or removed; a file of format zero also
/// loses the defaults a later build replaced. Returns the settings removed.
pub(crate) fn migrate(file: &mut Table, format: Format) -> Vec<Migrated> {
    let mut migrated: Vec<Migrated> = migrate_antialiasing_switch(file).into_iter().collect();
    migrated.extend(migrate_retired_key_actions(file));
    if format == Format::Zero {
        migrated.extend(migrate_single_pass_group(file));
        migrated.extend(remove_superseded(file, superseded()));
    }
    migrated
}

/// Replace the older `fsr.enabled` switch, in a file of any format, with the anti-aliasing choice
/// that superseded it: off becomes the engine's anti-aliasing, and on is the choice's default, FSR,
/// so it is left out. A file that already names the choice keeps it, and the switch is dropped.
fn migrate_antialiasing_switch(file: &mut Table) -> Option<Migrated> {
    let Some(Value::Table(fsr)) = file.get_mut(LEGACY_FSR_TABLE) else {
        return None;
    };
    let removed = fsr.remove(LEGACY_FSR_ENABLED)?;
    if removed == Value::Boolean(false) && !fsr.contains_key(ANTIALIASING_KEY) {
        fsr.insert(
            ANTIALIASING_KEY.to_owned(),
            Value::String(ENGINE_ANTIALIASING.to_owned()),
        );
    }
    Some(Migrated {
        path: format!("{LEGACY_FSR_TABLE}.{LEGACY_FSR_ENABLED}"),
        removed: removed.to_string(),
    })
}

/// Remove, from a file of any format, the director's key bindings whose action no longer exists
/// ([`RETIRED_KEY_ACTIONS`]), which would otherwise fail the bindings' deserialization and reset
/// every binding the file holds. A file of format zero then holds the stock layout of its day
/// without them, which [`superseded`] recognises.
fn migrate_retired_key_actions(file: &mut Table) -> Option<Migrated> {
    let Some(Value::Table(director)) = file.get_mut(DIRECTOR_TABLE) else {
        return None;
    };
    let Some(Value::Table(keymap)) = director.get_mut(KEYMAP_TABLE) else {
        return None;
    };
    let Some(Value::Array(bindings)) = keymap.get_mut(BINDINGS_KEY) else {
        return None;
    };
    let retired = |binding: &Value| {
        binding
            .get(ACTION_KEY)
            .and_then(Value::as_str)
            .is_some_and(|action| RETIRED_KEY_ACTIONS.contains(&action))
    };
    let (removed, kept): (Vec<Value>, Vec<Value>) =
        std::mem::take(bindings).into_iter().partition(retired);
    *bindings = kept;
    (!removed.is_empty()).then(|| Migrated {
        path: format!("{DIRECTOR_TABLE}.{KEYMAP_TABLE}.{BINDINGS_KEY}"),
        removed: Value::Array(removed).to_string(),
    })
}

/// Remove, from a file of format zero, the single-pass group's switches that hold their old default
/// (off), so that the group follows its current default as a whole.
/// The format did not write a setting's value as a choice, and the group was off when it was
/// current, so an off value there is the old default; a switch the file holds on is kept, and the
/// rest of the group then joins it.
fn migrate_single_pass_group(file: &mut Table) -> Vec<Migrated> {
    let mut migrated = Vec::new();
    let mut remove_if = |table: Option<&mut Value>, parent: &str, key: &str, old: &Value| {
        let Some(Value::Table(table)) = table else {
            return;
        };
        if table.get(key).is_some_and(|value| same(value, old))
            && let Some(removed) = table.remove(key)
        {
            migrated.push(Migrated {
                path: format!("{parent}.{key}"),
                removed: removed.to_string(),
            });
        }
    };
    let off = Value::Boolean(false);
    for key in SINGLE_PASS_GROUP_SWITCHES {
        let single_pass = match file.get_mut(STEREO_TABLE) {
            Some(Value::Table(stereo)) => stereo.get_mut(SINGLE_PASS_TABLE),
            _ => None,
        };
        remove_if(
            single_pass,
            &format!("{STEREO_TABLE}.{SINGLE_PASS_TABLE}"),
            key,
            &off,
        );
    }
    migrated
}

/// The director's key bindings, under their tables, and a binding's action key.
const DIRECTOR_TABLE: &str = "director";
const KEYMAP_TABLE: &str = "keymap";
const BINDINGS_KEY: &str = "bindings";
const ACTION_KEY: &str = "action";
/// The key actions a later build removed, as a settings file names them.
const RETIRED_KEY_ACTIONS: [&str; 2] = ["ToggleReplay", "ToggleScenario"];

/// The single-pass group's switches that were off by default when format zero was current, under
/// their table.
const STEREO_TABLE: &str = "stereo";
const SINGLE_PASS_TABLE: &str = "single_pass";
const SINGLE_PASS_GROUP_SWITCHES: [&str; 8] = [
    "enabled",
    "dual_eye",
    "collapse",
    "double_wide",
    "reproject",
    "terrain",
    "tree_impostors",
    "occluder",
];

/// The table and key of the switch the anti-aliasing choice superseded.
const LEGACY_FSR_TABLE: &str = "fsr";
const LEGACY_FSR_ENABLED: &str = "enabled";
/// The anti-aliasing choice's key in the `fsr` table, and its engine value as written.
const ANTIALIASING_KEY: &str = "antialiasing";
const ENGINE_ANTIALIASING: &str = "Engine";

/// The defaults a later build replaced, by setting: each list holds the values the setting's
/// default once had. Only numeric tunings are listed: a file of format zero that holds one of
/// their exact old values never had it chosen, as far as can be told, so [`migrate`] removes it. A
/// switch or a mode has too few values for that to hold (a player may have set it back to its old
/// default on purpose), so it is kept as written. A default changed after the current format was
/// introduced needs no entry here, since the current format does not write defaults.
const SUPERSEDED: &str = r#"
[controllers.body.rig.pelvis]
back_bend_deg = [20.0]

[controllers.body.rig.step]
settle_speed_mps = [0.12]
settle_done_m = [0.015]
settle_s = [1.0]

[controllers.body.rig.yaw]
moving_deadzone_deg = [15.0]
"#;

/// The superseded defaults, by path: [`SUPERSEDED`], and the director's key bindings as they stood
/// before the collider view moved to its own key.
fn superseded() -> Vec<(String, Vec<Value>)> {
    let mut entries = Vec::new();
    match toml::from_str::<Table>(SUPERSEDED) {
        Ok(table) => flatten(&table, String::new(), &mut entries),
        Err(error) => tracing::error!("settings: the superseded defaults do not parse: {error}"),
    }
    let mut keymap = Keymap::new();
    let bindings = keymap.bindings.to_mut();
    if let Some(binding) = bindings
        .iter_mut()
        .find(|binding| binding.action == Action::ToggleColliderView)
    {
        binding.chord = COLLIDER_VIEW_CHORD_BEFORE_F3;
        if let Ok(value) = Value::try_from(&keymap.bindings) {
            entries.push(("director.keymap.bindings".to_owned(), vec![value]));
        }
    }
    entries
}

/// The collider view's default chord before it moved to its own key: Ctrl and the number pad's
/// zero.
const COLLIDER_VIEW_CHORD_BEFORE_F3: Chord = Chord {
    key: VK_NUMPAD0.0,
    ctrl: true,
    shift: false,
    alt: false,
};

fn flatten(table: &Table, prefix: String, entries: &mut Vec<(String, Vec<Value>)>) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Table(child) => flatten(child, path, entries),
            Value::Array(values) => entries.push((path, values.clone())),
            other => entries.push((path, vec![other.clone()])),
        }
    }
}

#[cfg(test)]
mod tests {
    use jc3vr_core::config::{
        format::{Upgrade, VERSION, VERSION_KEY},
        load::LoadReport,
    };
    use jc3vr_tooling::director::keymap::Binding;
    use jc3vr_view::{fsr::AntiAliasing, stereo::config::set_single_pass_feature};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::config::load::parse;

    fn upgrade(file: &mut Table) -> Upgrade {
        engine::format::upgrade(&SCHEMA, file)
    }

    fn defaults() -> Value {
        Value::try_from(Config::new()).unwrap()
    }

    fn lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
        path.split('.')
            .try_fold(value, |value, key| value.as_table()?.get(key))
    }

    #[test]
    fn the_defaults_write_only_the_version() {
        assert_eq!(
            to_text(&Config::new()).unwrap().trim(),
            format!("{VERSION_KEY} = {VERSION}")
        );
    }

    #[test]
    fn a_save_writes_only_the_changed_settings() {
        let mut config = Config::new();
        config.controllers.body.rig.yaw.moving_deadzone_deg = 30.0;
        config.body.height = 1.9;
        config.vr.loader_path = Some("C:/openxr/openxr_loader.dll".to_owned());
        let text = to_text(&config).unwrap();
        assert!(
            text.starts_with(&format!("{VERSION_KEY} = {VERSION}\n")),
            "{text}"
        );
        assert!(text.contains("\nheight = 1.9\n"), "{text}");
        let file: Table = toml::from_str(&text).unwrap();
        let mut keys: Vec<&str> = file.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["body", "controllers", VERSION_KEY, "vr"]);
        let file = Value::Table(file);
        assert_eq!(
            lookup(&file, "controllers.body.rig.yaw")
                .and_then(Value::as_table)
                .map(Table::len),
            Some(1)
        );
        assert_eq!(
            lookup(&file, "body")
                .and_then(Value::as_table)
                .map(Table::len),
            Some(1)
        );
    }

    #[test]
    fn a_format_zero_file_drops_its_superseded_defaults_and_keeps_chosen_values() {
        let mut file: Table = toml::from_str(
            r#"
[controllers.body.rig.yaw]
moving_deadzone_deg = 15.0
turn_deg = 75.0
[controllers.body.rig.pelvis]
back_bend_deg = 12.0
"#,
        )
        .unwrap();
        let upgrade = upgrade(&mut file);
        assert_eq!(upgrade.format, Format::Zero);
        assert_eq!(
            upgrade.migrated,
            [Migrated {
                path: "controllers.body.rig.yaw.moving_deadzone_deg".to_owned(),
                removed: "15.0".to_owned(),
            }]
        );
        let file = Value::Table(file);
        assert!(lookup(&file, "controllers.body.rig.yaw.moving_deadzone_deg").is_none());
        assert_eq!(
            lookup(&file, "controllers.body.rig.yaw.turn_deg"),
            Some(&Value::Float(75.0))
        );
        assert_eq!(
            lookup(&file, "controllers.body.rig.pelvis.back_bend_deg"),
            Some(&Value::Float(12.0))
        );
    }

    #[test]
    fn a_sparse_file_loads_as_the_settings_it_was_written_from() {
        let mut config = Config::new();
        config.controllers.body.rig.yaw.moving_deadzone_deg = 15.0;
        config.controllers.weapons.wheel.enabled = true;
        config.body.height = 1.9;
        let text = to_text(&config).unwrap();
        let loaded = parse(&text).unwrap();
        assert_eq!(loaded.report, LoadReport::default());
        assert_eq!(
            toml::to_string(&loaded.config).unwrap(),
            toml::to_string(&config).unwrap()
        );
    }

    #[test]
    fn a_stale_default_in_an_old_file_loads_as_the_new_default() {
        let loaded = parse("[controllers.body.rig.yaw]\nmoving_deadzone_deg = 15.0\n").unwrap();
        assert_eq!(
            loaded.config.controllers.body.rig.yaw.moving_deadzone_deg,
            Config::new().controllers.body.rig.yaw.moving_deadzone_deg
        );
        assert_eq!(
            paths(&loaded.report.migrated),
            ["controllers.body.rig.yaw.moving_deadzone_deg"]
        );
    }

    fn paths(migrated: &[Migrated]) -> Vec<&str> {
        migrated.iter().map(|entry| entry.path.as_str()).collect()
    }

    #[test]
    fn a_format_zero_file_keeps_its_switches_and_modes_at_old_defaults() {
        let text = r#"
[controllers.weapons.wheel]
enabled = true
[controllers.body.rig.steps]
enabled = false
[controllers.overlay]
debug_gizmos = true
[controllers.physical_hands.grab_view]
enabled = false
[hud.atlas]
enabled = false
"#;
        let mut file: Table = toml::from_str(text).unwrap();
        let kept = file.clone();
        let upgrade = upgrade(&mut file);
        assert_eq!(upgrade.format, Format::Zero);
        assert_eq!(upgrade.migrated, []);
        assert_eq!(file, kept);
    }

    #[test]
    fn a_format_zero_file_takes_the_single_pass_group_whole() {
        let mut old = Config::new();
        set_single_pass_feature(&mut old, false);
        old.stereo.single_pass.enabled = true;
        let loaded = parse(&toml::to_string(&old).unwrap()).unwrap();
        assert_eq!(
            toml::Value::try_from(loaded.config.stereo.single_pass).unwrap(),
            toml::Value::try_from(Config::new().stereo.single_pass).unwrap()
        );
        let migrated = paths(&loaded.report.migrated);
        assert!(
            migrated.contains(&"stereo.single_pass.reproject"),
            "{migrated:?}"
        );
        assert!(
            !migrated.contains(&"stereo.single_pass.enabled"),
            "{migrated:?}"
        );
    }

    #[test]
    fn a_versioned_file_keeps_an_explicit_single_pass_choice() {
        let mut off = Config::new();
        set_single_pass_feature(&mut off, false);
        let text = to_text(&off).unwrap();
        assert!(text.contains("[stereo.single_pass]"), "{text}");
        assert!(text.contains("enabled = false"), "{text}");
        let loaded = parse(&text).unwrap();
        assert!(!loaded.config.stereo.single_pass.enabled);

        let partial = parse("version = 1\n[stereo.single_pass]\nenabled = true\n").unwrap();
        assert_eq!(
            toml::Value::try_from(partial.config.stereo.single_pass).unwrap(),
            toml::Value::try_from(Config::new().stereo.single_pass).unwrap()
        );
    }

    #[test]
    fn a_versioned_file_keeps_every_value() {
        let text = "version = 1\n[controllers.body.rig.yaw]\nmoving_deadzone_deg = 15.0\n";
        let mut file: Table = toml::from_str(text).unwrap();
        let upgrade = upgrade(&mut file);
        assert_eq!(upgrade.format, Format::Current);
        assert!(upgrade.migrated.is_empty());
        assert!(!file.contains_key(VERSION_KEY));
        assert!(
            lookup(
                &Value::Table(file),
                "controllers.body.rig.yaw.moving_deadzone_deg"
            )
            .is_some()
        );

        let mut newer: Table = toml::from_str("version = 99\n").unwrap();
        assert_eq!(upgrade_format(&mut newer), Format::Newer(99));
        let mut unreadable: Table = toml::from_str("version = \"one\"\n").unwrap();
        assert_eq!(upgrade_format(&mut unreadable), Format::Unreadable);
    }

    fn upgrade_format(file: &mut Table) -> Format {
        upgrade(file).format
    }

    #[test]
    fn the_old_fsr_switch_becomes_the_antialiasing_choice() {
        let off = parse("version = 1\n[fsr]\nenabled = false\n").unwrap();
        assert_eq!(off.config.fsr.antialiasing, AntiAliasing::Engine);
        assert_eq!(paths(&off.report.migrated), ["fsr.enabled"]);
        let on = parse("[fsr]\nenabled = true\n").unwrap();
        assert_eq!(on.config.fsr.antialiasing, AntiAliasing::Fsr);
        let chosen =
            parse("version = 1\n[fsr]\nenabled = false\nantialiasing = \"Taa\"\n").unwrap();
        assert_eq!(chosen.config.fsr.antialiasing, AntiAliasing::Taa);
        let mut config = Config::new();
        config.fsr.antialiasing = AntiAliasing::Taa;
        let text = to_text(&config).unwrap();
        assert!(text.contains("antialiasing = \"Taa\""), "{text}");
        assert_eq!(
            parse(&text).unwrap().config.fsr.antialiasing,
            AntiAliasing::Taa
        );
    }

    #[test]
    fn the_old_collider_view_key_follows_the_new_default() {
        let mut stale = Config::new();
        let bindings = stale.director.keymap.bindings.to_mut();
        let binding = bindings
            .iter_mut()
            .find(|binding| binding.action == Action::ToggleColliderView)
            .unwrap();
        binding.chord = COLLIDER_VIEW_CHORD_BEFORE_F3;
        let mut file: Table = toml::from_str(&toml::to_string(&stale).unwrap()).unwrap();
        let upgrade = upgrade(&mut file);
        assert_eq!(paths(&upgrade.migrated), ["director.keymap.bindings"]);
    }

    #[test]
    fn a_retired_key_action_is_dropped_and_the_other_bindings_kept() {
        let mut file: Table = toml::from_str(
            r#"
version = 1
[[director.keymap.bindings]]
action = "ToggleScenario"
chord = { key = 109, ctrl = true }
[[director.keymap.bindings]]
action = "TogglePause"
chord = { key = 120 }
"#,
        )
        .unwrap();
        let upgrade = upgrade(&mut file);
        assert_eq!(paths(&upgrade.migrated), ["director.keymap.bindings"]);
        let loaded = parse(&toml::to_string(&file).unwrap()).unwrap();
        assert!(loaded.report.reset.is_empty(), "{:?}", loaded.report.reset);
        assert_eq!(
            loaded.config.director.keymap.bindings.as_ref(),
            [Binding {
                action: Action::TogglePause,
                chord: Chord {
                    key: 120,
                    ctrl: false,
                    shift: false,
                    alt: false,
                },
            }]
        );
    }

    #[test]
    fn every_superseded_default_names_a_numeric_setting_whose_default_changed() {
        let defaults = defaults();
        let entries = superseded();
        assert!(entries.len() > 5, "{}", entries.len());
        for (path, values) in entries {
            let current =
                lookup(&defaults, &path).unwrap_or_else(|| panic!("{path} is not a setting"));
            assert!(!values.is_empty(), "{path}");
            for value in &values {
                assert!(
                    !same(value, current),
                    "{path}: {value} is the current default"
                );
                assert_eq!(
                    std::mem::discriminant(value),
                    std::mem::discriminant(current),
                    "{path}: {value} is not of the setting's type"
                );
                assert!(
                    matches!(value, Value::Float(_) | Value::Integer(_) | Value::Array(_)),
                    "{path}: {value} is a switch or a mode, which a player may have chosen"
                );
            }
        }
    }
}
