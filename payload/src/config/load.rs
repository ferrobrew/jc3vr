//! The tolerant load: turning a settings file written by any build into a [`Config`].
//!
//! The configuration types change as settings are added, renamed, and removed, and the file can be
//! edited by hand, so the load bridges drift in both directions rather than rejecting the file:
//!
//! 1. The file is parsed into a TOML value tree and merged over the defaults serialized the same
//!    way. A setting the file lacks takes its default; a setting the file has that the defaults do
//!    not stays in the tree, since it is either an optional setting (a `None` default is left out
//!    of the defaults' tree as it is left out of the file) or one this build does not know.
//! 2. The tree is deserialized. Settings this build does not know are skipped and reported. A
//!    value that cannot be read (the wrong type, an unknown enum variant, a list of the wrong
//!    length) fails the deserialization at a path; that one value is replaced by its default (or
//!    removed, when it has none) and the deserialization is tried again, a bounded number of
//!    times. One bad value costs one setting, not the file.
//! 3. The settings whose types admit unusable values are validated ([`crate::config::validate`]),
//!    The checks name the setting that fails, so that setting is reset to its default (or the
//!    group around it, when the default fails too) and the check runs again; a subtree is reset
//!    whole only when that does not make it pass.
//!
//! Lists merge element by element when the file's list is as long as the default one (a vector
//! or a fixed-size array); a list of another length replaces the default whole, so a list that can
//! grow or shrink (the director's key bindings) loads as written.

use std::fmt;

use serde::{Serialize, de::DeserializeOwned};
use toml::{Table, Value};

use crate::{
    config::{Config, validate},
    controllers::config::{ControllersConfig, SightTable},
    headpose::config::VrTurnConfig,
};

/// A loaded configuration and what the load had to change to get it.
pub(crate) struct Loaded {
    pub config: Config,
    pub report: LoadReport,
}

/// The settings a load defaulted, skipped, or reset, by path (`controllers.hands.grip`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LoadReport {
    /// Settings the file lacked, which took their defaults.
    pub defaulted: Vec<String>,
    /// Settings the file had that this build does not know, which were skipped.
    pub ignored: Vec<String>,
    /// Settings the file had that could not be used, which were reset to their defaults.
    pub reset: Vec<Reset>,
}

/// A setting, or a group of settings, reset to its defaults, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reset {
    pub path: String,
    pub reason: String,
}

/// Why a settings file could not be loaded at all.
#[derive(Debug)]
pub(crate) enum LoadError {
    /// The file is not a TOML document.
    Syntax(toml::de::Error),
    /// The defaults could not be serialized to merge the file over; a programming error.
    Defaults(toml::ser::Error),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(source) => write!(f, "settings: the file is not valid TOML: {source}"),
            Self::Defaults(source) => {
                write!(f, "settings: could not serialize the defaults: {source}")
            }
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Syntax(source) => Some(source),
            Self::Defaults(source) => Some(source),
        }
    }
}

impl LoadReport {
    pub fn is_empty(&self) -> bool {
        self.defaulted.is_empty() && self.ignored.is_empty() && self.reset.is_empty()
    }

    /// Log what the load changed, against the file at `path`.
    pub fn log(&self, path: &str) {
        if !self.defaulted.is_empty() {
            tracing::info!(
                path,
                defaulted = ?self.defaulted,
                "settings: the file lacks some settings; they take their defaults",
            );
        }
        if !self.ignored.is_empty() {
            tracing::warn!(
                path,
                ignored = ?self.ignored,
                "settings: the file has settings this build does not know; they are skipped",
            );
        }
        for reset in &self.reset {
            tracing::warn!(
                path,
                setting = reset.path,
                "settings: reset to the default: {}",
                reset.reason,
            );
        }
    }
}

/// Load a settings file's text: parse it, merge it over the defaults, deserialize it resetting the
/// values that cannot be read, and validate it. Fails only when the text is not TOML.
pub(crate) fn parse(text: &str) -> Result<Loaded, LoadError> {
    let file: Table = toml::from_str(text).map_err(LoadError::Syntax)?;
    let defaults = Value::try_from(Config::new()).map_err(LoadError::Defaults)?;
    let mut report = LoadReport::default();
    let mut merged = merge(
        defaults.clone(),
        Value::Table(file),
        &[],
        &mut report.defaulted,
    );
    let mut config = None;
    for _ in 0..MAX_RESETS {
        match deserialize(&merged) {
            Ok((loaded, ignored)) => {
                report.ignored = ignored;
                config = Some(loaded);
                break;
            }
            Err(error) => {
                let steps = error_steps(error.path());
                let reason = error.into_inner().message().to_owned();
                match reset(&mut merged, &defaults, &steps) {
                    Some(path) => report.reset.push(Reset { path, reason }),
                    None => break,
                }
            }
        }
    }
    let mut config = config.unwrap_or_else(|| {
        report.reset.push(Reset {
            path: WHOLE_FILE.to_owned(),
            reason: "too many settings could not be read".to_owned(),
        });
        Config::new()
    });
    validate_subtrees(&mut config, &mut report);
    Ok(Loaded { config, report })
}

/// How many values a load resets before it gives up on the file.
const MAX_RESETS: usize = 64;

/// The path [`Reset`] names when the whole file was replaced by the defaults.
const WHOLE_FILE: &str = "(the whole file)";

/// One step of a path into the value tree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Key(String),
    Index(usize),
}

/// Overlay `actual` on `defaults`: tables merge key by key, lists of the same length element by
/// element, and any other value in `actual` replaces the default. Keys only `defaults` has are
/// pushed to `defaulted`; keys only `actual` has are kept for the deserializer to use or skip.
fn merge(defaults: Value, actual: Value, path: &[Step], defaulted: &mut Vec<String>) -> Value {
    match (defaults, actual) {
        (Value::Table(mut defaults), Value::Table(actual)) => {
            defaulted.extend(
                defaults
                    .keys()
                    .filter(|key| !actual.contains_key(*key))
                    .map(|key| display(&child(path, Step::Key(key.clone())))),
            );
            for (key, value) in actual {
                let slot = match defaults.remove(&key) {
                    Some(default) => merge(
                        default,
                        value,
                        &child(path, Step::Key(key.clone())),
                        defaulted,
                    ),
                    None => value,
                };
                defaults.insert(key, slot);
            }
            Value::Table(defaults)
        }
        (Value::Array(defaults), Value::Array(actual)) if defaults.len() == actual.len() => {
            Value::Array(
                defaults
                    .into_iter()
                    .zip(actual)
                    .enumerate()
                    .map(|(index, (default, value))| {
                        merge(default, value, &child(path, Step::Index(index)), defaulted)
                    })
                    .collect(),
            )
        }
        (_, actual) => actual,
    }
}

/// Deserialize the merged tree, collecting the paths of the settings this build does not know.
fn deserialize(
    merged: &Value,
) -> Result<(Config, Vec<String>), serde_path_to_error::Error<toml::de::Error>> {
    let mut ignored = Vec::new();
    let mut record = |path: serde_ignored::Path<'_>| {
        let mut steps = Vec::new();
        ignored_steps(&path, &mut steps);
        ignored.push(display(&steps));
    };
    let config = serde_path_to_error::deserialize(serde_ignored::Deserializer::new(
        merged.clone(),
        &mut record,
    ))?;
    Ok((config, ignored))
}

/// The path of a deserialization error, up to the first step that is not a key or an index.
fn error_steps(path: &serde_path_to_error::Path) -> Vec<Step> {
    path.iter()
        .map_while(|segment| match segment {
            serde_path_to_error::Segment::Map { key } => Some(Step::Key(key.clone())),
            serde_path_to_error::Segment::Seq { index } => Some(Step::Index(*index)),
            serde_path_to_error::Segment::Enum { .. } | serde_path_to_error::Segment::Unknown => {
                None
            }
        })
        .collect()
}

/// Reset the value at `steps` in `merged`: replace it with the value at the same place in
/// `defaults`, or remove it when the defaults have none there (an optional setting, or a list
/// element past the default list's end). When neither applies, or the value already is the
/// default, try the enclosing value. Returns the path reset, or `None` when the error is at the
/// root and nothing can be reset.
fn reset(merged: &mut Value, defaults: &Value, steps: &[Step]) -> Option<String> {
    for len in (1..=steps.len()).rev() {
        let target = &steps[..len];
        if let Some(default) = lookup(defaults, target) {
            if let Some(slot) = lookup_mut(merged, target)
                && slot != default
            {
                *slot = default.clone();
                return Some(display(target));
            }
            continue;
        }
        let (last, parent) = target.split_last()?;
        let removed = match (lookup_mut(merged, parent), last) {
            (Some(Value::Table(table)), Step::Key(key)) => table.remove(key).is_some(),
            (Some(Value::Array(array)), Step::Index(index)) if *index < array.len() => {
                array.remove(*index);
                true
            }
            _ => false,
        };
        if removed {
            return Some(display(target));
        }
    }
    None
}

/// Reset the settings that fail their checks. The controller and VR turn checks name the setting
/// that fails, so that setting is reset alone (or the group around it, when its default does not
/// pass either), and the subtree is reset whole only when that does not make it pass. The sight
/// corrections are measured rather than chosen, so a failure elsewhere in the controller settings
/// keeps them.
fn validate_subtrees(config: &mut Config, report: &mut LoadReport) {
    let fail = |report: &mut LoadReport, path: &str, error: validate::Invalid| {
        report.reset.push(Reset {
            path: path.to_owned(),
            reason: error.to_string(),
        });
    };
    if let Err(error) = validate::sights(&config.controllers.aim.sights) {
        config.controllers.aim.sights = SightTable::EMPTY;
        fail(report, "controllers.aim.sights", error);
    }
    if let Err(error) = reset_named(
        &mut config.controllers,
        &ControllersConfig::new(),
        "controllers",
        validate::controllers,
        report,
    ) {
        let sights = config.controllers.aim.sights;
        config.controllers = ControllersConfig::new();
        config.controllers.aim.sights = sights;
        fail(report, "controllers", error);
    }
    if let Err(error) = reset_named(
        &mut config.headpose.vr_turn,
        &VrTurnConfig::new(),
        "headpose.vr_turn",
        validate::vr_turn,
        report,
    ) {
        config.headpose.vr_turn = VrTurnConfig::new();
        fail(report, "headpose.vr_turn", error);
    }
    if let Err(error) = config.body.validate() {
        config.body = Config::new().body;
        fail(report, "body", validate::Invalid(error.to_string()));
    }
}

/// Reset the settings of `value` that `check` names, one at a time, until it passes. `prefix` is
/// `value`'s path in the configuration, and each reset is reported under it. A setting is replaced
/// by its value in `defaults`, or, when it already has that value, the group enclosing it is
/// ([`reset`]). Returns the check's error when a failure names no setting that can be reset, or
/// the resets run out, so that the caller resets `value` whole.
fn reset_named<T: Serialize + DeserializeOwned>(
    value: &mut T,
    defaults: &T,
    prefix: &str,
    check: impl Fn(&T) -> Result<(), validate::Invalid>,
    report: &mut LoadReport,
) -> Result<(), validate::Invalid> {
    let Ok(defaults) = Value::try_from(defaults) else {
        return check(value);
    };
    for _ in 0..MAX_RESETS {
        let Err(error) = check(value) else {
            return Ok(());
        };
        let Ok(mut tree) = Value::try_from(&*value) else {
            return Err(error);
        };
        let Some(path) = reset(&mut tree, &defaults, &named_steps(&error.0)) else {
            return Err(error);
        };
        let Ok(reset_value) = tree.try_into::<T>() else {
            return Err(error);
        };
        *value = reset_value;
        report.reset.push(Reset {
            path: format!("{prefix}.{path}"),
            reason: error.to_string(),
        });
    }
    check(value)
}

/// The path a validation message leads with ([`validate::Invalid`]), in `a.b[2].c` form. A
/// message that does not lead with one gives a path that names nothing in the tree, which
/// [`reset`] does not reset.
fn named_steps(message: &str) -> Vec<Step> {
    let Some(token) = message.split_whitespace().next() else {
        return Vec::new();
    };
    let mut steps = Vec::new();
    for segment in token.trim_end_matches([':', ',']).split('.') {
        let (key, mut indices) = segment.split_once('[').unwrap_or((segment, ""));
        if key.is_empty() {
            return Vec::new();
        }
        steps.push(Step::Key(key.to_owned()));
        while let Some((index, rest)) = indices.split_once(']') {
            let Ok(index) = index.parse() else {
                return steps;
            };
            steps.push(Step::Index(index));
            indices = rest.strip_prefix('[').unwrap_or(rest);
        }
    }
    steps
}

fn child(path: &[Step], step: Step) -> Vec<Step> {
    let mut child = path.to_vec();
    child.push(step);
    child
}

fn ignored_steps(path: &serde_ignored::Path<'_>, steps: &mut Vec<Step>) {
    match path {
        serde_ignored::Path::Root => {}
        serde_ignored::Path::Seq { parent, index } => {
            ignored_steps(parent, steps);
            steps.push(Step::Index(*index));
        }
        serde_ignored::Path::Map { parent, key } => {
            ignored_steps(parent, steps);
            steps.push(Step::Key(key.clone()));
        }
        serde_ignored::Path::Some { parent }
        | serde_ignored::Path::NewtypeStruct { parent }
        | serde_ignored::Path::NewtypeVariant { parent } => ignored_steps(parent, steps),
    }
}

fn lookup<'a>(value: &'a Value, steps: &[Step]) -> Option<&'a Value> {
    steps
        .iter()
        .try_fold(value, |value, step| match (value, step) {
            (Value::Table(table), Step::Key(key)) => table.get(key),
            (Value::Array(array), Step::Index(index)) => array.get(*index),
            _ => None,
        })
}

fn lookup_mut<'a>(value: &'a mut Value, steps: &[Step]) -> Option<&'a mut Value> {
    steps
        .iter()
        .try_fold(value, |value, step| match (value, step) {
            (Value::Table(table), Step::Key(key)) => table.get_mut(key),
            (Value::Array(array), Step::Index(index)) => array.get_mut(*index),
            _ => None,
        })
}

/// A path in `controllers.hands.grip.position_m[0]` form.
fn display(steps: &[Step]) -> String {
    let mut path = String::new();
    for step in steps {
        match step {
            Step::Key(key) => {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(key);
            }
            Step::Index(index) => path.push_str(&format!("[{index}]")),
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn key(key: &str) -> Step {
        Step::Key(key.to_owned())
    }

    #[test]
    fn a_validation_message_names_its_setting() {
        assert_eq!(
            named_steps("hands.reach_t must be finite and in [0, 1], got 5"),
            [key("hands"), key("reach_t")]
        );
        assert_eq!(
            named_steps("weapons.grenade.warning_interval_s[2] must be finite"),
            [
                key("weapons"),
                key("grenade"),
                key("warning_interval_s"),
                Step::Index(2)
            ]
        );
        assert_eq!(named_steps("a.b: bad"), [key("a"), key("b")]);
        assert_eq!(named_steps(""), []);
        assert_eq!(named_steps("..."), []);
    }
}
