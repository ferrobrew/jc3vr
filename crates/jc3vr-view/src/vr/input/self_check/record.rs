//! What the input self-check has already run against: each OpenXR runtime and set of controller
//! interaction profiles it has checked, kept in `input_self_check.toml` in the configuration
//! directory beside the settings ([`jc3vr_core::config::paths`]), so that the check runs once
//! per new runtime or controller rather than every session.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use jc3vr_core::config::{paths, write};
use serde::{Deserialize, Serialize};

/// A runtime and the controllers the check ran against: the runtime's name (its version is left out,
/// so a runtime update does not run the check again) and the distinct interaction profile paths
/// bound to the hands, in a canonical order. A hand without a profile is left out, so a check with
/// one controller on and one with both on the same profile share a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CheckKey {
    runtime: String,
    profiles: Vec<String>,
}

impl CheckKey {
    pub(crate) fn new(runtime: String, profiles: impl IntoIterator<Item = String>) -> CheckKey {
        let mut profiles: Vec<String> = profiles.into_iter().collect();
        profiles.sort();
        profiles.dedup();
        CheckKey { runtime, profiles }
    }
}

impl fmt::Display for CheckKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} with {}", self.runtime, self.profiles.join(", "))
    }
}

/// Whether the check runs for a runtime and controllers.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    Run,
    /// The record holds the key: an earlier session checked this runtime and these controllers.
    Skip,
}

/// Whether to run the check. A requested run always runs, as does one whose runtime or controllers
/// could not be named (`key` is `None`); otherwise it runs unless the record holds the key.
pub(crate) fn decide(record: &Record, key: Option<&CheckKey>, requested: bool) -> Decision {
    match key {
        Some(key) if !requested && record.contains(key) => Decision::Skip,
        _ => Decision::Run,
    }
}

/// The checked runtimes and controllers.
#[derive(Debug, Default)]
pub(crate) struct Record {
    checked: Vec<CheckKey>,
    /// Set when the file exists but could not be read or parsed. While it is, the file is never
    /// written, so that a damaged record is left for the player to inspect rather than replaced.
    load_error: Option<RecordError>,
}

impl Record {
    /// The record in the configuration directory, or an empty one when there is no directory.
    pub(crate) fn load() -> Record {
        match path() {
            Some(path) => Record::load_from(&path),
            None => Record::default(),
        }
    }

    /// Note that the check ran for `key`, and save the record.
    pub(crate) fn note(&mut self, key: CheckKey) {
        if self.contains(&key) {
            return;
        }
        self.checked.push(key);
        if let Some(path) = path() {
            self.save_to(&path);
        }
    }

    fn contains(&self, key: &CheckKey) -> bool {
        self.checked.contains(key)
    }

    /// Load the record at `path`. A missing file is an empty record; an unreadable or unparsable
    /// one is an empty record that refuses to save.
    fn load_from(path: &Path) -> Record {
        let parsed = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str::<RecordFile>(&text).map_err(|error| RecordError {
                path: path.to_owned(),
                kind: RecordErrorKind::Parse(error.to_string()),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RecordFile::default()),
            Err(error) => Err(RecordError {
                path: path.to_owned(),
                kind: RecordErrorKind::Read(error.to_string()),
            }),
        };
        match parsed {
            Ok(file) => Record {
                checked: file
                    .checked
                    .into_iter()
                    .map(|key| CheckKey::new(key.runtime, key.profiles))
                    .collect(),
                load_error: None,
            },
            Err(error) => {
                tracing::warn!(target: "vr", "{error}; the file is left untouched");
                Record {
                    checked: Vec::new(),
                    load_error: Some(error),
                }
            }
        }
    }

    fn save_to(&self, path: &Path) {
        if let Some(error) = &self.load_error {
            tracing::warn!(target: "vr", "{error}; the check is remembered for this session only");
            return;
        }
        let file = RecordFile {
            checked: self.checked.clone(),
        };
        let result = toml::to_string(&file)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                write::replace(path, text.as_bytes()).map_err(|error| error.to_string())
            });
        if let Err(error) = result {
            tracing::warn!(target: "vr", "input self-check record: could not save: {error}");
        }
    }
}

/// The record file's contents.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct RecordFile {
    checked: Vec<CheckKey>,
}

/// Why the record file could not be loaded.
#[derive(Debug, Clone)]
struct RecordError {
    path: PathBuf,
    kind: RecordErrorKind,
}

#[derive(Debug, Clone)]
enum RecordErrorKind {
    Read(String),
    Parse(String),
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        match &self.kind {
            RecordErrorKind::Read(error) => {
                write!(f, "input self-check record: could not read {path}: {error}")
            }
            RecordErrorKind::Parse(error) => {
                write!(
                    f,
                    "input self-check record: could not parse {path}: {error}"
                )
            }
        }
    }
}

impl std::error::Error for RecordError {}

/// The record file.
fn path() -> Option<PathBuf> {
    paths::file("input_self_check.toml")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const INDEX: &str = "/interaction_profiles/valve/index_controller";
    const TOUCH: &str = "/interaction_profiles/oculus/touch_controller";

    fn key(runtime: &str, profiles: &[&str]) -> CheckKey {
        CheckKey::new(runtime.to_owned(), profiles.iter().map(|&p| p.to_owned()))
    }

    fn record(keys: &[CheckKey]) -> Record {
        Record {
            checked: keys.to_vec(),
            load_error: None,
        }
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("jc3vr-self-check-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    #[test]
    fn a_new_runtime_or_controller_runs_and_a_checked_one_skips() {
        let checked = record(&[key("Runtime A", &[INDEX])]);
        assert_eq!(
            decide(&checked, Some(&key("Runtime A", &[INDEX])), false),
            Decision::Skip
        );
        assert_eq!(
            decide(&checked, Some(&key("Runtime B", &[INDEX])), false),
            Decision::Run
        );
        assert_eq!(
            decide(&checked, Some(&key("Runtime A", &[TOUCH])), false),
            Decision::Run
        );
        assert_eq!(
            decide(&checked, Some(&key("Runtime A", &[INDEX, TOUCH])), false),
            Decision::Run
        );
    }

    #[test]
    fn a_requested_run_or_an_unnamed_runtime_always_runs() {
        let checked = record(&[key("Runtime A", &[INDEX])]);
        assert_eq!(
            decide(&checked, Some(&key("Runtime A", &[INDEX])), true),
            Decision::Run
        );
        assert_eq!(decide(&checked, None, false), Decision::Run);
    }

    #[test]
    fn the_key_ignores_hand_order_and_a_repeated_profile() {
        assert_eq!(
            key("Runtime A", &[TOUCH, INDEX]),
            key("Runtime A", &[INDEX, TOUCH])
        );
        assert_eq!(
            key("Runtime A", &[INDEX, INDEX]),
            key("Runtime A", &[INDEX])
        );
    }

    #[test]
    fn a_noted_check_is_remembered_across_loads() {
        let path = scratch_dir("remembered").join("input_self_check.toml");
        let mut first = Record::load_from(&path);
        assert_eq!(first.checked, Vec::new());
        first.checked.push(key("Runtime A", &[INDEX]));
        first.save_to(&path);
        let second = Record::load_from(&path);
        assert_eq!(
            decide(&second, Some(&key("Runtime A", &[INDEX])), false),
            Decision::Skip
        );
    }

    #[test]
    fn an_unparsable_record_runs_the_check_and_is_never_overwritten() {
        let path = scratch_dir("unparsable").join("input_self_check.toml");
        std::fs::write(&path, "checked = 3").expect("write the fixture");
        let mut record = Record::load_from(&path);
        assert!(record.load_error.is_some());
        let key = key("Runtime A", &[INDEX]);
        assert_eq!(decide(&record, Some(&key), false), Decision::Run);
        record.checked.push(key);
        record.save_to(&path);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read the fixture"),
            "checked = 3"
        );
    }
}
