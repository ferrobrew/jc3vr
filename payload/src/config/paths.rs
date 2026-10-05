//! Where the payload keeps its files: the per-user configuration directory that
//! [`directories::ProjectDirs`] names for jc3vr. On Windows that is under the roaming application
//! data folder; under Proton it is inside the game's prefix. The settings file lives there, beside
//! the files kept apart from it (the debug UI's dock layout and the director's teleport bookmarks).

use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use directories::ProjectDirs;

/// The settings file's name.
pub(crate) const SETTINGS_FILE_NAME: &str = "settings.toml";

/// The configuration directory, created on first use. `None` when the system names no home
/// directory to put it under.
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

fn resolve() -> Option<PathBuf> {
    let Some(dirs) = ProjectDirs::from("me", "philpax", "jc3vr") else {
        tracing::warn!(
            "settings: the system names no home directory; settings and layouts are not saved"
        );
        return None;
    };
    let dir = dirs.config_dir().to_owned();
    if let Err(error) = fs::create_dir_all(&dir) {
        // Kept anyway: each read and write reports its own failure against the path.
        tracing::warn!(
            "settings: could not create the configuration directory {}: {error}",
            dir.display()
        );
    }
    Some(dir)
}
