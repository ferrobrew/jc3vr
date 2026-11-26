//! The body rig's tuning file as the evaluator reads it: the repository's
//! `crates/jc3vr-body/tuning/rig.toml`, read when a command runs rather than built in, so an edit to
//! it applies to the next run without a rebuild.
//!
//! The evaluator takes the pure core's table (`core`, [`CoreTuning`]) and the grounded rig's
//! restart after a spell off the ground. The file is not part of a recording: a replay runs the
//! repository's current values, where the player's settings (the pelvis, step, and yaw tuning) come
//! from the recording or `--tuning`. The payload validates the file at load, and the body crate's
//! tests validate the shipped one, so the evaluator only parses it.

use std::{
    fmt, io,
    path::{Path, PathBuf},
};

use body_rig::CoreTuning;
use serde::Deserialize;

/// What the evaluator reads from the rig's tuning file.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RigFile {
    /// The pure core's tuning that the settings do not hold.
    pub core: CoreTuning,
    /// A spell off the ground longer than this starts the rig again, nanoseconds: the grounded
    /// rig's `grounded.resume_after_ms`.
    pub resume_after_ns: u64,
}

impl RigFile {
    /// The repository's file.
    pub fn load() -> Result<Self, RigFileError> {
        Self::load_from(&repository_path())
    }

    /// The file at `path`.
    pub fn load_from(path: &Path) -> Result<Self, RigFileError> {
        let error = |kind| RigFileError {
            path: path.to_owned(),
            kind,
        };
        let text = std::fs::read_to_string(path).map_err(|e| error(RigFileErrorKind::Read(e)))?;
        let file: FileForm =
            toml::from_str(&text).map_err(|e| error(RigFileErrorKind::Parse(Box::new(e))))?;
        Ok(Self {
            core: file.core,
            resume_after_ns: file.grounded.resume_after_ms.saturating_mul(1_000_000),
        })
    }
}

/// Where the repository keeps the rig's tuning file.
pub fn repository_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/jc3vr-body/tuning/rig.toml")
}

/// The rig's tuning file could not be read.
#[derive(Debug)]
pub struct RigFileError {
    pub path: PathBuf,
    pub kind: RigFileErrorKind,
}

#[derive(Debug)]
pub enum RigFileErrorKind {
    /// The file could not be read.
    Read(io::Error),
    /// The file is not TOML, or its `core` table or `grounded.resume_after_ms` does not match.
    Parse(Box<toml::de::Error>),
}

impl fmt::Display for RigFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        match &self.kind {
            RigFileErrorKind::Read(error) => {
                write!(f, "rig tuning: could not read {path}: {error}")
            }
            RigFileErrorKind::Parse(error) => write!(f, "rig tuning: {path}: {error}"),
        }
    }
}

impl std::error::Error for RigFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            RigFileErrorKind::Read(error) => Some(error),
            RigFileErrorKind::Parse(error) => Some(error.as_ref()),
        }
    }
}

/// The part of the file the evaluator reads; the payload's other tables are left alone.
#[derive(Deserialize)]
struct FileForm {
    core: CoreTuning,
    grounded: GroundedForm,
}

#[derive(Deserialize)]
struct GroundedForm {
    resume_after_ms: u64,
}
