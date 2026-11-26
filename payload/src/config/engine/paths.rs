//! Where the payload keeps its files: the directory named by `JC3VR_CONFIG_DIR`, or the per-user
//! configuration directory that [`directories::ProjectDirs`] names for jc3vr when it is unset.
//! The settings file lives beside the debug UI's dock layout and the director's teleport bookmarks.

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use directories::ProjectDirs;

use crate::session::wine::{self, ConversionError};

/// The settings file's name.
pub(crate) const SETTINGS_FILE_NAME: &str = "settings.toml";

/// The configuration directory, created on first use. `None` when no usable directory is available.
pub(crate) fn dir() -> Option<&'static Path> {
    DIR.get_or_init(resolve).as_deref()
}

/// A file named `name` in the configuration directory.
pub(crate) fn file(name: &str) -> Option<PathBuf> {
    Some(dir()?.join(name))
}

/// The settings file.
pub(crate) fn settings_file() -> Option<PathBuf> {
    file(SETTINGS_FILE_NAME)
}

static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
const ENV_VAR: &str = "JC3VR_CONFIG_DIR";

fn resolve() -> Option<PathBuf> {
    let override_value = std::env::var_os(ENV_VAR);
    let dir = match resolve_path(
        override_value.as_deref(),
        || ProjectDirs::from("me", "philpax", "jc3vr").map(|dirs| dirs.config_dir().to_owned()),
        wine::dos_file_name,
    ) {
        Ok(dir) => dir,
        Err(error) => {
            tracing::warn!("{error}; configuration files are not read or saved");
            return None;
        }
    };
    if let Err(error) = fs::create_dir_all(&dir) {
        tracing::warn!(
            "settings: could not create the configuration directory {}: {error}",
            dir.display()
        );
        if override_value.is_some() {
            return None;
        }
    }
    Some(dir)
}

fn resolve_path(
    override_value: Option<&OsStr>,
    default: impl FnOnce() -> Option<PathBuf>,
    unix_to_windows: impl FnOnce(&str) -> Result<PathBuf, ConversionError>,
) -> Result<PathBuf, PathError> {
    let Some(value) = override_value else {
        return default().ok_or(PathError::NoHome);
    };
    let value = value
        .to_str()
        .ok_or(PathError::InvalidOverride("the value is not Unicode"))?
        .trim();
    if value.is_empty() {
        return Err(PathError::InvalidOverride("the value is empty"));
    }
    if value.contains('\0') {
        return Err(PathError::InvalidOverride("the value contains a NUL"));
    }
    let path = if value.starts_with('/') {
        unix_to_windows(value).map_err(PathError::Conversion)?
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() {
        return Err(PathError::InvalidOverride("the path is not absolute"));
    }
    Ok(path)
}

#[derive(Debug, PartialEq, Eq)]
enum PathError {
    NoHome,
    InvalidOverride(&'static str),
    Conversion(ConversionError),
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoHome => write!(f, "settings: the system names no home directory"),
            Self::InvalidOverride(reason) => write!(f, "settings: {ENV_VAR}: {reason}"),
            Self::Conversion(error) => write!(f, "settings: {ENV_VAR}: {error}"),
        }
    }
}

impl std::error::Error for PathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Conversion(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt as _};

    use pretty_assertions::assert_eq;
    use test_case::test_case;

    use super::*;

    #[test]
    fn unset_uses_the_default_without_conversion() {
        let default = PathBuf::from(r"C:\Users\fixture\AppData\Roaming\jc3vr\config");
        assert_eq!(
            resolve_path(
                None,
                || Some(default.clone()),
                |_| panic!("unexpected conversion")
            ),
            Ok(default)
        );
    }

    #[test]
    fn unset_without_a_home_disables_configuration() {
        assert_eq!(
            resolve_path(None, || None, |_| panic!("unexpected conversion")),
            Err(PathError::NoHome)
        );
    }

    #[test_case(r"D:\harness\config" ; "drive")]
    #[test_case("Z:/tmp/harness/config" ; "wine drive")]
    #[test_case(r"\\server\share\config" ; "share")]
    fn windows_override_does_not_resolve_the_default(value: &str) {
        assert_eq!(
            resolve_path(
                Some(OsStr::new(value)),
                || panic!("unexpected default"),
                |_| panic!("unexpected conversion"),
            ),
            Ok(PathBuf::from(value))
        );
    }

    #[test_case("/tmp/harness/config/" ; "absolute")]
    #[test_case("//tmp/harness/config/" ; "double leading slash")]
    fn unix_override_uses_wine_conversion(value: &str) {
        assert_eq!(
            resolve_path(
                Some(OsStr::new(value)),
                || panic!("unexpected default"),
                |path| {
                    assert_eq!(path, value);
                    Ok(PathBuf::from(r"Z:\tmp\harness\config"))
                },
            ),
            Ok(PathBuf::from(r"Z:\tmp\harness\config"))
        );
    }

    #[test_case("" ; "empty")]
    #[test_case("  \t " ; "blank")]
    #[test_case("relative/config" ; "relative")]
    #[test_case(r"C:config" ; "drive relative")]
    #[test_case(r"\config" ; "rooted without a drive")]
    #[test_case("C:\\config\0ignored" ; "nul")]
    fn invalid_override_never_uses_the_default(value: &str) {
        assert!(matches!(
            resolve_path(
                Some(OsStr::new(value)),
                || panic!("unexpected default"),
                |_| panic!("unexpected conversion"),
            ),
            Err(PathError::InvalidOverride(_))
        ));
    }

    #[test]
    fn non_unicode_override_never_uses_the_default() {
        let value = OsString::from_wide(&[0xd800]);
        assert!(matches!(
            resolve_path(
                Some(&value),
                || panic!("unexpected default"),
                |_| panic!("unexpected conversion"),
            ),
            Err(PathError::InvalidOverride(_))
        ));
    }

    #[test_case(ConversionError::NotWine ; "not wine")]
    #[test_case(ConversionError::Refused ; "conversion refused")]
    fn failed_conversion_never_uses_the_default(error: ConversionError) {
        assert_eq!(
            resolve_path(
                Some(OsStr::new("/tmp/harness/config")),
                || panic!("unexpected default"),
                |_| Err(error.clone()),
            ),
            Err(PathError::Conversion(error))
        );
    }

    #[test]
    fn relative_conversion_result_is_rejected() {
        assert!(matches!(
            resolve_path(
                Some(OsStr::new("/tmp/harness/config")),
                || panic!("unexpected default"),
                |_| Ok(PathBuf::from("relative")),
            ),
            Err(PathError::InvalidOverride(_))
        ));
    }
}
