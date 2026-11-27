//! Why the tuning could not be loaded.

use std::{fmt, io, path::Path};

use crate::{config::Invalid, session::wine::ConversionError};

/// Why one tuning file, or the directory the files are read from, could not be used.
#[derive(Debug)]
pub enum TuningError {
    /// The payload could not name its own DLL, beside which the packaged files sit.
    NoPayloadPath,
    /// A source-tree path could not be converted to the Windows path the payload opens.
    Conversion {
        path: String,
        source: ConversionError,
    },
    /// A file could not be read or did not hold valid tuning.
    File { path: String, cause: FileCause },
    /// The watcher thread could not be started.
    Watcher(io::Error),
}

impl fmt::Display for TuningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tuning: ")?;
        match self {
            Self::NoPayloadPath => write!(
                f,
                "could not name the payload's DLL, beside which the tuning files sit"
            ),
            Self::Conversion { path, source } => write!(f, "{path}: {source}"),
            Self::File { path, cause } => write!(f, "{path}: {cause}"),
            Self::Watcher(error) => write!(f, "could not start the file watcher: {error}"),
        }
    }
}

impl std::error::Error for TuningError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoPayloadPath => None,
            Self::Conversion { source, .. } => Some(source),
            Self::File { cause, .. } => Some(cause),
            Self::Watcher(error) => Some(error),
        }
    }
}

/// Why a tuning file's text could not be used.
#[derive(Debug)]
pub enum FileCause {
    /// No file is there.
    Missing,
    /// The file is there but could not be read.
    Read(io::Error),
    /// The text is not TOML, or does not match the file's root: a key is missing, unknown, or of
    /// the wrong type.
    Syntax(toml::de::Error),
    /// A value is out of range or inconsistent with another.
    Invalid(Invalid),
}

impl FileCause {
    pub(crate) fn read(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            Self::Missing
        } else {
            Self::Read(error)
        }
    }

    pub(crate) fn at(self, path: &Path) -> TuningError {
        TuningError::File {
            path: path.display().to_string(),
            cause: self,
        }
    }
}

impl fmt::Display for FileCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "the file is missing"),
            Self::Read(error) => write!(f, "could not read the file: {error}"),
            // The TOML error spans several lines (the position, the offending line, and a caret),
            // so it starts on its own.
            Self::Syntax(error) => write!(f, "the file does not match its tuning:\n{error}"),
            Self::Invalid(Invalid(message)) => write!(f, "invalid value: {message}"),
        }
    }
}

impl std::error::Error for FileCause {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Missing | Self::Invalid(_) => None,
            Self::Read(error) => Some(error),
            Self::Syntax(error) => Some(error),
        }
    }
}

/// Why the payload's startup stopped: every tuning file that could not be loaded.
#[derive(Debug)]
pub struct StartupError {
    pub errors: Vec<TuningError>,
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, error) in self.errors.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for StartupError {}
