//! Where the tuning files are read from.

use std::path::{Path, PathBuf};

use crate::{
    session::wine,
    tuning::{error::TuningError, file::Location},
};

/// Where the tuning files are read from, chosen by the export the injector starts the payload with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The `tuning/` directory beside the payload's DLL, where the build stages every crate's files
    /// under the crate's name: what a packaged mod reads.
    Packaged,
    /// The source tree the payload was built from: each crate's own `tuning/` directory, so an edit
    /// to the repository's file applies live. The development launchers ask for it.
    SourceTree,
}

impl Source {
    /// The path the payload opens for the file at `location`. `packaged_root` is the packaged
    /// `tuning/` directory, which only [`Source::Packaged`] reads.
    pub fn path(self, location: &Location, packaged_root: &Path) -> Result<PathBuf, TuningError> {
        match self {
            Self::Packaged => Ok(location.packaged(packaged_root)),
            Self::SourceTree => {
                let path = location.source_tree();
                // A build made on the Linux host names its source tree by a host path, which wine
                // maps into the Windows namespace.
                if path.starts_with('/') {
                    wine::dos_file_name(&path)
                        .map_err(|source| TuningError::Conversion { path, source })
                } else {
                    Ok(PathBuf::from(path))
                }
            }
        }
    }
}

/// The packaged `tuning/` directory: beside the payload's DLL.
pub(crate) fn packaged_root() -> Result<PathBuf, TuningError> {
    crate::module::get_path()
        .and_then(|dll| Some(dll.parent()?.join("tuning")))
        .ok_or(TuningError::NoPayloadPath)
}
