//! Replacing a file whole: the contents go to a fresh temporary file beside it, which is then moved
//! over it, so a failed write never leaves a truncated file behind.

use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Why a file could not be replaced.
#[derive(Debug)]
pub(crate) enum WriteError {
    /// The path has no parent directory or no file name.
    InvalidPath(PathBuf),
    /// A file operation failed.
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(path) => write!(f, "{} is not a file path", path.display()),
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "could not {operation} {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidPath(_) => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Replace the file at `path` with `contents`.
pub(crate) fn replace(path: &Path, contents: &[u8]) -> Result<(), WriteError> {
    let invalid_path = || WriteError::InvalidPath(path.to_owned());
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
                return Err(WriteError::Io {
                    operation: "create",
                    path: temporary,
                    source,
                });
            }
        }
    };
    let io_error = |operation, source| WriteError::Io {
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
            replace_path(&temporary, path).map_err(|source| WriteError::Io {
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

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
