//! The settings file's format: which settings a save writes, and the version that heads the file.
//!
//! A save writes only the settings that differ from their defaults, headed by the format's version
//! ([`to_text`]). A setting left at its default is absent from the file, so it follows the default
//! when a later build changes it, while a value the player chose stays as written.
//!
//! Format zero, a file without a version, holds every setting, each default as it stood when the
//! file was written, so a default changed since is indistinguishable from the player's choice.
//! [`upgrade`] takes the version out and hands the file to the schema's migration
//! ([`ConfigRoot::migrate`](crate::config::engine::ConfigRoot::migrate)), which can remove such
//! defaults ([`remove_superseded`]) so that they take the current ones.

use std::fmt;

use toml::{Table, Value};

use crate::config::engine::store::{Root, Schema};

/// The format a save writes.
pub const VERSION: i64 = 1;

/// The key the format's version is stored under, at the top of the file.
pub const VERSION_KEY: &str = "version";

/// What [`upgrade`] found and did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upgrade {
    pub format: Format,
    /// The settings that held a superseded default and were removed.
    pub migrated: Vec<Migrated>,
}

/// A setting that held a superseded default and was removed, so that it takes the current one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migrated {
    /// The setting's path (`controllers.body.rig.yaw.moving_deadzone_deg`).
    pub path: String,
    /// The value the file held, as TOML.
    pub removed: String,
}

/// The format a file was written in.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Format {
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
pub fn to_text(schema: &Schema, root: &Root) -> Result<String, TextError> {
    let actual = tree(schema, root)?;
    let defaults = tree(schema, schema.defaults().as_ref())?;
    let mut file = Table::new();
    file.insert(VERSION_KEY.to_owned(), Value::Integer(VERSION));
    if let Some(Value::Table(changed)) = difference(actual, &defaults) {
        file.extend(changed);
    }
    toml::to_string(&file).map_err(TextError::Serialize)
}

/// Why the settings could not be written as text.
#[derive(Debug)]
pub enum TextError {
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

/// Take the version out of a settings file's table, and run the schema's migration on it.
pub fn upgrade(schema: &Schema, file: &mut Table) -> Upgrade {
    let format = match file.remove(VERSION_KEY) {
        None => Format::Zero,
        Some(Value::Integer(version)) if version > VERSION => Format::Newer(version),
        Some(Value::Integer(version)) if version >= 1 => Format::Current,
        Some(Value::Integer(_)) => Format::Zero,
        Some(_) => Format::Unreadable,
    };
    let migrated = schema.migrate(file, format);
    Upgrade { format, migrated }
}

/// Remove from `file` each setting that holds one of its superseded defaults, given by path
/// (`controllers.body.rig.yaw.moving_deadzone_deg`) in `entries`, so that it takes the current
/// default. Returns the settings removed, in `entries`' order.
pub fn remove_superseded(file: &mut Table, entries: Vec<(String, Vec<Value>)>) -> Vec<Migrated> {
    let mut migrated = Vec::new();
    for (path, superseded) in entries {
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
            && let Some(removed) = table.remove(*last)
        {
            migrated.push(Migrated {
                path,
                removed: removed.to_string(),
            });
        }
    }
    migrated
}

/// Whether two values are the same setting value. Floats compare at the precision the settings
/// hold them, so a value written from an `f32` matches the same number written by hand.
pub fn same(a: &Value, b: &Value) -> bool {
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

/// The configuration's value tree, through its text: a float serialized straight into the tree
/// widens from `f32` and prints with the widening's digits (`1.8489999771118164`), while the text
/// prints the `f32` as written (`1.849`).
fn tree(schema: &Schema, root: &Root) -> Result<Value, TextError> {
    let text = schema.serialize(root).map_err(TextError::Serialize)?;
    toml::from_str::<Table>(&text)
        .map(Value::Table)
        .map_err(TextError::Reparse)
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
