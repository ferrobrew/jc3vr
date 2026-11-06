//! The settings file's format: which settings a save writes, and the upgrade of a file an older
//! format wrote.
//!
//! A save writes only the settings that differ from their defaults, headed by the format's version
//! ([`to_text`]). A setting left at its default is absent from the file, so it follows the default
//! when a later build changes it, while a value the player chose stays as written.
//!
//! Format zero, a file without a version, holds every setting, each default as it stood when the
//! file was written, so a default changed since is indistinguishable from the player's choice.
//! [`upgrade`] removes each setting of such a file that holds a default a later build replaced
//! ([`SUPERSEDED`]), so that it takes the current default, as it would have had the file been
//! written in the current format.

use std::fmt;

use toml::{Table, Value};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_NUMPAD0;

use crate::{
    config::Config,
    director::keymap::{Action, Chord, Keymap},
};

/// The format a save writes.
pub(crate) const VERSION: i64 = 1;

/// The key the format's version is stored under, at the top of the file.
pub(crate) const VERSION_KEY: &str = "version";

/// What [`upgrade`] found and did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Upgrade {
    pub format: Format,
    /// The settings that held a superseded default and were removed, by path.
    pub migrated: Vec<String>,
}

/// The format a file was written in.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Format {
    /// No version: every setting, with the defaults of its day.
    Zero,
    /// The format this build writes.
    Current,
    /// A version this build does not know, from a newer build. It loads as the current format.
    Newer(i64),
    /// A version key that is not an integer. It loads as the current format, so that nothing is
    /// migrated on a guess.
    Unreadable,
}

impl Format {
    /// Whether the file holds every setting, so that one it lacks is worth reporting.
    pub fn complete(self) -> bool {
        self == Self::Zero
    }
}

/// The configuration as a save writes it: the version, then each setting that differs from its
/// default. A list that differs is written whole.
pub(crate) fn to_text(config: &Config) -> Result<String, TextError> {
    let actual = tree(config)?;
    let defaults = tree(&Config::new())?;
    let mut file = Table::new();
    file.insert(VERSION_KEY.to_owned(), Value::Integer(VERSION));
    if let Some(Value::Table(changed)) = difference(actual, &defaults) {
        file.extend(changed);
    }
    toml::to_string(&file).map_err(TextError::Serialize)
}

/// Why the settings could not be written as text.
#[derive(Debug)]
pub(crate) enum TextError {
    /// The configuration could not be serialized.
    Serialize(toml::ser::Error),
    /// The serialized configuration did not parse back; a programming error.
    Reparse(toml::de::Error),
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize(source) => write!(f, "format: could not serialize: {source}"),
            Self::Reparse(source) => {
                write!(f, "format: the serialized settings do not parse: {source}")
            }
        }
    }
}

impl std::error::Error for TextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialize(source) => Some(source),
            Self::Reparse(source) => Some(source),
        }
    }
}

/// The configuration's value tree, through its text: a float serialized straight into the tree
/// widens from `f32` and prints with the widening's digits (`1.8489999771118164`), while the text
/// prints the `f32` as written (`1.849`).
fn tree(config: &Config) -> Result<Value, TextError> {
    let text = toml::to_string(config).map_err(TextError::Serialize)?;
    toml::from_str::<Table>(&text)
        .map(Value::Table)
        .map_err(TextError::Reparse)
}

/// Take the version out of a settings file's table, and remove the superseded defaults from a file
/// of format zero.
pub(crate) fn upgrade(file: &mut Table) -> Upgrade {
    let format = match file.remove(VERSION_KEY) {
        None => Format::Zero,
        Some(Value::Integer(version)) if version > VERSION => Format::Newer(version),
        Some(Value::Integer(version)) if version >= 1 => Format::Current,
        Some(Value::Integer(_)) => Format::Zero,
        Some(_) => Format::Unreadable,
    };
    let mut migrated = Vec::new();
    if format == Format::Zero {
        for (path, superseded) in superseded() {
            let steps: Vec<&str> = path.split('.').collect();
            let Some((last, parents)) = steps.split_last() else {
                continue;
            };
            let Some(table) =
                parents
                    .iter()
                    .try_fold(&mut *file, |table, key| match table.get_mut(*key) {
                        Some(Value::Table(child)) => Some(child),
                        _ => None,
                    })
            else {
                continue;
            };
            if table
                .get(*last)
                .is_some_and(|value| superseded.iter().any(|old| same(value, old)))
            {
                table.remove(*last);
                migrated.push(path);
            }
        }
    }
    Upgrade { format, migrated }
}

/// The part of `actual` that differs from `defaults`, or `None` when nothing does. Tables differ
/// key by key; any other value differs whole.
fn difference(actual: Value, defaults: &Value) -> Option<Value> {
    match (actual, defaults) {
        (Value::Table(actual), Value::Table(defaults)) => {
            let changed: Table = actual
                .into_iter()
                .filter_map(|(key, value)| {
                    let changed = match defaults.get(&key) {
                        Some(default) => difference(value, default),
                        None => Some(value),
                    };
                    changed.map(|value| (key, value))
                })
                .collect();
            (!changed.is_empty()).then_some(Value::Table(changed))
        }
        (actual, defaults) => (!same(&actual, defaults)).then_some(actual),
    }
}

/// Whether two values are the same setting value. Floats compare at the precision the settings
/// hold them, so a value written from an `f32` matches the same number written by hand.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(a), Value::Float(b)) => (*a as f32) == (*b as f32),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (Value::Table(a), Value::Table(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| same(a, b)))
        }
        (a, b) => a == b,
    }
}

/// The defaults a later build replaced, by setting: each list holds the values the setting's
/// default once had. A file of format zero that holds one of them never had it chosen, as far as
/// can be told, so [`upgrade`] removes it. A default changed after the current format was
/// introduced needs no entry here, since the current format does not write defaults.
const SUPERSEDED: &str = r#"
[controllers.body.procedural]
mode = ["Off"]
blend_s = [0.3]
chest_clear_back_m = [0.05]

[controllers.body.rig.pelvis]
back_bend_deg = [20.0]

[controllers.body.rig.step]
settle_speed_mps = [0.12]
settle_done_m = [0.015]
settle_s = [1.0]

[controllers.body.rig.yaw]
moving_deadzone_deg = [15.0]

[controllers.overlay]
ghost_hands = [true]
debug_gizmos = [true]

[controllers.physical_hands.bodies.brace]
distance_m = [0.002]
angle_rad = [0.01]

[controllers.physical_hands.climb]
enabled = [false]

[controllers.physical_hands.grab_view]
wrist = [true]

[controllers.vehicles.wingsuit_gestures]
flight_angle_deg = [35.0]
brake_hysteresis_deg = [12.0]

[controllers.weapons.hold]
grip_to_hold = [false]

[controllers.weapons.holsters]
stick_click_cycles = [true]

[controllers.weapons.reload]
flick_eject = [true]

[controllers.weapons.snatch.yank]
min_distance_m = [0.12]
min_speed_mps = [1.5, 1.0]

[controllers.weapons.wheel]
enabled = [true]

[hud.atlas]
enabled = [false]

[hud.world_markers]
enabled = [false]
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
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::config::load::{LoadReport, parse};

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
            ["controllers.body.rig.yaw.moving_deadzone_deg"]
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
            loaded.report.migrated,
            ["controllers.body.rig.yaw.moving_deadzone_deg"]
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
        assert_eq!(upgrade.migrated, ["director.keymap.bindings"]);
    }

    #[test]
    fn every_superseded_default_names_a_setting_whose_default_changed() {
        let defaults = defaults();
        let entries = superseded();
        assert!(entries.len() > 20, "{}", entries.len());
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
            }
        }
    }
}
