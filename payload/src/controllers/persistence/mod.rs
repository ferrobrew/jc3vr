//! Explicit saving and loading of controller tuning (`controllers.local.json`).
//!
//! The file holds the whole [`ControllersConfig`] and the shared VR turn settings, serialized
//! through the runtime configuration types themselves, beside a format version. Because those types
//! change as controller settings are added, renamed, and removed, loading is tolerant of both
//! directions of drift: the file is merged over the current defaults, so a setting the file lacks
//! keeps its default and a setting the payload no longer has is ignored, and both are reported. The
//! version changes only when the layout changes in a way a merge cannot bridge. The merged file is
//! validated completely before either the controller settings or the turn settings change.
//!
//! The file lives outside the build output so that `cargo clean` does not delete it (see
//! [`file_path`]). Player body dimensions remain owned by `body.local.json`.

use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    config::Config, controllers::config::ControllersConfig, headpose::config::VrTurnConfig,
};

mod validate;
mod worker;

pub use worker::shutdown;
pub(crate) use worker::{Request, Status, queue, start_pending, status};

/// The settings file's name.
pub const FILE_NAME: &str = "controllers.local.json";
/// The current on-disk format version.
pub const FILE_VERSION: u32 = 1;

/// The versioned controller settings file.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct ControllerFile {
    /// The file format version, not the payload version.
    pub version: u32,
    /// The controller settings, including tuning and sight calibration.
    pub controllers: ControllersConfig,
    /// Settings shared with the headpose subsystem.
    pub headpose: HeadposeFile,
}

/// The part of the headpose configuration owned by this file.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HeadposeFile {
    /// The VR body-turn settings also used by the controller right stick.
    pub vr_turn: VrTurnConfig,
}

/// A parsed and validated file, with the settings the merge over the defaults filled or dropped.
pub struct Parsed {
    pub file: ControllerFile,
    pub report: ShapeReport,
}

/// The paths of settings a loaded file lacked (filled from the defaults) or had that the payload
/// does not know (ignored), in `controllers.hands.grip` form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShapeReport {
    pub defaulted: Vec<String>,
    pub ignored: Vec<String>,
}

impl ShapeReport {
    pub fn is_empty(&self) -> bool {
        self.defaulted.is_empty() && self.ignored.is_empty()
    }
}

/// Why a controller settings operation failed.
#[derive(Debug)]
pub enum Error {
    /// The payload's own path, from which the file's is derived, is not available.
    PathUnavailable,
    /// The file's path has no parent directory or file name.
    InvalidPath(PathBuf),
    /// A file operation failed.
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    /// The file is not valid JSON.
    Parse(serde_json::Error),
    /// The file has no integer top-level `version`.
    MissingVersion,
    /// The file has a version this payload does not understand.
    UnknownVersion(u64),
    /// A setting has a value of the wrong type, after merging over the defaults.
    Deserialize(serde_json::Error),
    /// The settings could not be serialized.
    Serialize(serde_json::Error),
    /// A value is well-typed but out of range or inconsistent.
    Invalid(String),
    /// The worker thread could not be started.
    Spawn(io::Error),
    /// The worker thread panicked.
    WorkerPanicked,
    /// Payload shutdown cancelled the operation.
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "controller settings: ")?;
        match self {
            Self::PathUnavailable => write!(f, "the payload path is unknown"),
            Self::InvalidPath(path) => write!(f, "{} is not a file path", path.display()),
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "could not {operation} {}: {source}", path.display()),
            Self::Parse(source) => write!(f, "the file is not valid JSON: {source}"),
            Self::MissingVersion => {
                write!(f, "the top-level `version` is missing or is not an integer")
            }
            Self::UnknownVersion(version) => write!(
                f,
                "unsupported file version {version} (expected {FILE_VERSION})"
            ),
            Self::Deserialize(source) => write!(f, "a setting has the wrong type: {source}"),
            Self::Serialize(source) => write!(f, "could not serialize the settings: {source}"),
            Self::Invalid(detail) => write!(f, "invalid value: {detail}"),
            Self::Spawn(source) => write!(f, "could not start the settings worker: {source}"),
            Self::WorkerPanicked => write!(f, "the settings worker panicked"),
            Self::Cancelled => write!(f, "shutdown cancelled the operation"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } | Self::Spawn(source) => Some(source),
            Self::Parse(source) | Self::Deserialize(source) | Self::Serialize(source) => {
                Some(source)
            }
            Self::PathUnavailable
            | Self::InvalidPath(_)
            | Self::MissingVersion
            | Self::UnknownVersion(_)
            | Self::Invalid(_)
            | Self::WorkerPanicked
            | Self::Cancelled => None,
        }
    }
}

/// Where the settings are saved: the directory that holds the build output directory when the
/// payload runs from a Cargo `target/` tree (the workspace root, beside `body.local.json` copies
/// kept there), otherwise the payload DLL's own directory. See [`settings_dir`].
pub fn file_path() -> Option<PathBuf> {
    Some(settings_dir(&dll_dir()?).join(FILE_NAME))
}

/// Where a load reads from: the saved file, or, when there is none yet, a file left beside the
/// payload DLL, where earlier payloads saved it.
pub fn load_path() -> Option<PathBuf> {
    let path = file_path()?;
    if path.exists() {
        return Some(path);
    }
    let legacy = dll_dir()?.join(FILE_NAME);
    Some(if legacy.exists() { legacy } else { path })
}

/// Build the persisted representation from the runtime configuration.
pub fn from_config(config: &Config) -> ControllerFile {
    ControllerFile {
        version: FILE_VERSION,
        controllers: config.controllers,
        headpose: HeadposeFile {
            vr_turn: config.headpose.vr_turn,
        },
    }
}

/// Parse a file, merge it over the defaults, and validate it, without changing runtime state.
pub fn parse(text: &str) -> Result<Parsed, Error> {
    let value: Value = serde_json::from_str(text).map_err(Error::Parse)?;
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(Error::MissingVersion)?;
    if version != u64::from(FILE_VERSION) {
        return Err(Error::UnknownVersion(version));
    }
    let defaults = serde_json::to_value(from_config(&Config::new())).map_err(Error::Serialize)?;
    let mut report = ShapeReport::default();
    let merged = merge(defaults, value, "", &mut report);
    let file: ControllerFile = serde_json::from_value(merged).map_err(Error::Deserialize)?;
    validate::validate(&file)?;
    Ok(Parsed { file, report })
}

/// Apply a previously parsed file after validating it again at the mutation boundary.
pub fn apply(file: &ControllerFile, config: &mut Config) -> Result<(), Error> {
    validate::validate(file)?;
    config.controllers = file.controllers;
    config.headpose.vr_turn = file.headpose.vr_turn;
    Ok(())
}

/// Save an immutable snapshot without accessing the live configuration.
pub fn save_snapshot_to_path(path: &Path, file: &ControllerFile) -> Result<(), Error> {
    validate::validate(file)?;
    let json = serde_json::to_string_pretty(file).map_err(Error::Serialize)?;
    atomic_replace(path, json.as_bytes())
}

fn dll_dir() -> Option<PathBuf> {
    Some(crate::module::get_path()?.parent()?.to_owned())
}

/// The directory above the nearest `target` ancestor of `dll_dir`, or `dll_dir` itself when it is
/// not inside one. A local build runs the payload from `target/<triple>/<profile>/`, which
/// `cargo clean` deletes; the workspace root survives it.
fn settings_dir(dll_dir: &Path) -> &Path {
    dll_dir
        .ancestors()
        .find(|dir| {
            dir.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("target"))
        })
        .and_then(Path::parent)
        .unwrap_or(dll_dir)
}

/// Overlay `actual` on `defaults`: objects merge key by key and arrays element by element, and any
/// other value in `actual` replaces the default. Keys and elements only `defaults` has are reported
/// as defaulted, and those only `actual` has as ignored.
fn merge(defaults: Value, actual: Value, path: &str, report: &mut ShapeReport) -> Value {
    match (defaults, actual) {
        (Value::Object(mut defaults), Value::Object(mut actual)) => {
            for (key, slot) in defaults.iter_mut() {
                let child = child_path(path, key);
                match actual.remove(key) {
                    Some(value) => *slot = merge(slot.take(), value, &child, report),
                    None => report.defaulted.push(child),
                }
            }
            report
                .ignored
                .extend(actual.keys().map(|key| child_path(path, key)));
            Value::Object(defaults)
        }
        (Value::Array(mut defaults), Value::Array(actual)) => {
            let provided = actual.len();
            for (index, value) in actual.into_iter().enumerate() {
                let child = format!("{path}[{index}]");
                match defaults.get_mut(index) {
                    Some(slot) => *slot = merge(slot.take(), value, &child, report),
                    None => report.ignored.push(child),
                }
            }
            report
                .defaulted
                .extend((provided..defaults.len()).map(|index| format!("{path}[{index}]")));
            Value::Array(defaults)
        }
        (_, actual) => actual,
    }
}

fn child_path(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn atomic_replace(path: &Path, contents: &[u8]) -> Result<(), Error> {
    let invalid_path = || Error::InvalidPath(path.to_owned());
    let parent = path.parent().ok_or_else(invalid_path)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(invalid_path)?;
    let (temporary, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(Error::Io {
                    operation: "create",
                    path: temporary,
                    source,
                });
            }
        }
    };
    let io_error = |operation, source| Error::Io {
        operation,
        path: temporary.clone(),
        source,
    };
    let result = file
        .write_all(contents)
        .map_err(|source| io_error("write", source))
        .and_then(|()| file.sync_all().map_err(|source| io_error("flush", source)))
        .and_then(|()| {
            drop(file);
            replace_path(&temporary, path).map_err(|source| Error::Io {
                operation: "replace",
                path: path.to_owned(),
                source,
            })
        });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn replace_path(temporary: &Path, destination: &Path) -> io::Result<()> {
    use std::{iter, os::windows::ffi::OsStrExt};

    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };

    let source: Vec<u16> = temporary
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let target: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    // SAFETY: both paths are NUL-terminated wide strings that outlive the call.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(target.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(io::Error::from)
}

#[cfg(not(windows))]
fn replace_path(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(test)]
mod tests;
