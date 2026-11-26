//! The sessions root: the folder that holds every session's folder. It comes from the
//! `JC3VR_SESSIONS_DIR` environment variable when that is set, and otherwise is `sessions/` in the
//! per-user local data directory that [`directories::ProjectDirs`] names for jc3vr, beside the
//! settings' configuration directory.
//!
//! Under wine the variable usually holds a Unix path, since the launch scripts set it on the host;
//! wine's path conversion ([`crate::session::wine`]) turns it into the Windows path the payload
//! opens. A Windows path is taken as it is. The root's Unix form is kept beside its Windows form
//! when it is known (the variable named it, or wine could name it), so tools on the host can be
//! pointed at a session's files.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::session::wine::{self, ConversionError};

/// The environment variable naming the sessions root.
pub const ENV_VAR: &str = "JC3VR_SESSIONS_DIR";

/// A resolved sessions root.
#[derive(Debug, Clone)]
pub struct Root {
    /// The folder as the payload opens it.
    pub windows: PathBuf,
    /// The folder as the host names it, under wine; `None` on Windows or when wine could not name it.
    pub unix: Option<String>,
    pub source: Source,
}

/// Where the root came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The environment variable.
    Environment,
    /// The local data directory.
    LocalData,
}

/// Why the environment variable's root, or the default one, could not be used.
#[derive(Debug, Clone)]
pub enum RootError {
    /// The variable's value is not valid Unicode.
    NotUnicode(String),
    /// The variable holds a Unix path that wine could not convert.
    Conversion {
        path: String,
        source: ConversionError,
    },
    /// The system names no home directory for the local data directory.
    NoHome,
}

impl std::fmt::Display for RootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sessions root: ")?;
        match self {
            Self::NotUnicode(value) => write!(f, "{ENV_VAR} is not valid Unicode: {value}"),
            Self::Conversion { path, source } => {
                write!(f, "{ENV_VAR} names the Unix path {path}, but {source}")
            }
            Self::NoHome => write!(
                f,
                "the system names no home directory to keep sessions under"
            ),
        }
    }
}

impl std::error::Error for RootError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Conversion { source, .. } => Some(source),
            Self::NotUnicode(_) | Self::NoHome => None,
        }
    }
}

/// The outcome of resolving the root: the root used, if any, and the problem that moved it off the
/// environment variable's, if one did.
#[derive(Debug)]
pub struct Resolution {
    pub root: Result<Root, RootError>,
    pub env_problem: Option<RootError>,
}

/// Resolve the root from the environment, falling back to the local data directory when the
/// variable is unset, empty, or unusable. Creates the root, so wine can name its Unix form.
pub fn resolve() -> Resolution {
    let (env, env_problem) = match from_env() {
        Ok(root) => (root, None),
        Err(error) => (None, Some(error)),
    };
    let root = match env {
        Some(root) => Ok(root),
        None => local_data(),
    };
    Resolution { root, env_problem }
}

/// How the variable's value is read.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EnvPath<'a> {
    Unset,
    /// A host path, for wine to convert.
    Unix(&'a str),
    Windows(&'a str),
}

/// Classify the variable's value: blank is unset, a leading `/` is a Unix path (on Windows a
/// rooted path names a drive letter or a share), and anything else is a Windows path.
pub(crate) fn classify(value: &str) -> EnvPath<'_> {
    let value = value.trim();
    if value.is_empty() {
        EnvPath::Unset
    } else if value.starts_with('/') {
        EnvPath::Unix(trim_unix(value))
    } else {
        EnvPath::Windows(value)
    }
}

/// Join a relative path onto a Unix path, with exactly one `/` between them.
pub(crate) fn join_unix(base: &str, relative: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        relative.trim_start_matches('/')
    )
}

fn from_env() -> Result<Option<Root>, RootError> {
    let Some(value) = std::env::var_os(ENV_VAR) else {
        return Ok(None);
    };
    let value = value
        .into_string()
        .map_err(|value| RootError::NotUnicode(value.to_string_lossy().into_owned()))?;
    match classify(&value) {
        EnvPath::Unset => Ok(None),
        EnvPath::Unix(path) => {
            let windows = wine::dos_file_name(path).map_err(|source| RootError::Conversion {
                path: path.to_owned(),
                source,
            })?;
            create(&windows);
            Ok(Some(Root {
                windows,
                unix: Some(path.to_owned()),
                source: Source::Environment,
            }))
        }
        EnvPath::Windows(path) => Ok(Some(created_root(PathBuf::from(path), Source::Environment))),
    }
}

fn local_data() -> Result<Root, RootError> {
    let dirs = ProjectDirs::from("me", "philpax", "jc3vr").ok_or(RootError::NoHome)?;
    Ok(created_root(
        dirs.data_local_dir().join("sessions"),
        Source::LocalData,
    ))
}

fn created_root(windows: PathBuf, source: Source) -> Root {
    create(&windows);
    let unix = wine::unix_file_name(&windows).ok();
    Root {
        windows,
        unix,
        source,
    }
}

/// Create the root ahead of the first session file, so wine can name it. A failure is not fatal
/// here: the first artifact write reports it against the path.
fn create(path: &Path) {
    let _ = std::fs::create_dir_all(path);
}

/// A Unix path without trailing separators, keeping `/` itself.
fn trim_unix(path: &str) -> &str {
    match path.trim_end_matches('/') {
        "" => "/",
        trimmed => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use test_case::test_case;

    use super::*;

    #[test_case("", EnvPath::Unset ; "empty")]
    #[test_case("   ", EnvPath::Unset ; "blank")]
    #[test_case("/home/user/.local/share/jc3vr/sessions", EnvPath::Unix("/home/user/.local/share/jc3vr/sessions") ; "unix")]
    #[test_case("/data/sessions/", EnvPath::Unix("/data/sessions") ; "unix trailing slash")]
    #[test_case("/", EnvPath::Unix("/") ; "unix root")]
    #[test_case("D:\\jc3vr\\sessions", EnvPath::Windows("D:\\jc3vr\\sessions") ; "windows drive")]
    #[test_case("Z:/home/user/sessions", EnvPath::Windows("Z:/home/user/sessions") ; "windows forward slashes")]
    #[test_case("\\\\server\\share", EnvPath::Windows("\\\\server\\share") ; "windows share")]
    fn the_variable_is_classified_by_its_leading_character(value: &str, expected: EnvPath<'_>) {
        assert_eq!(classify(value), expected);
    }

    #[test_case("/a/b", "c/d", "/a/b/c/d" ; "plain")]
    #[test_case("/a/b/", "c", "/a/b/c" ; "trailing base slash")]
    #[test_case("/", "c", "/c" ; "root base")]
    fn unix_joins_have_one_separator(base: &str, relative: &str, expected: &str) {
        assert_eq!(join_unix(base, relative), expected);
    }
}
