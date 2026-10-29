//! The per-session output folder: one stamped folder beside the payload DLL under which every disk
//! artifact the mod produces is grouped, so a run's outputs share an unambiguous provenance.
//!
//! The root is resolved once, at startup ([`init`], before logging installs), so every artifact of a
//! run lands under the same `sessions/<session stamp>/` folder. Every stamp is a [`stamp`], the local
//! wall clock as `YYYY-MM-DD_HH-MM-SS_mmm`.
//!
//! What lands where is a closed set, in three shapes:
//!
//! - [`SessionFile`]: one file per session in the root, under a fixed name: `session.log` (the
//!   tracing log), `crash.log` (the crash handler's log), and `markers.jsonl` (take markers made with
//!   no video recording running).
//! - [`CaptureFile`]: one file per capture, `<folder>/<stamp>[_<qualifier>].<ext>`:
//!   `profiles/<stamp>.json` (profiler captures), `screenshots/<stamp>.png` with a `<stamp>.json`
//!   sidecar (F12), `grapple-logs/<stamp>.csv` (grapple telemetry), `recordings/<stamp>.jc3trk` and
//!   `recordings/<stamp>_lookback.jc3trk` (tracking recordings and lookback saves),
//!   `scenarios/<stamp>_<scenario file>.toml` (scenario summaries), and `shader-dumps/<stamp>.txt`
//!   (the vertex-shader name census).
//! - [`CaptureFolder`]: several files per capture, `<folder>/<stamp>/` holding files named by role:
//!   `traces/<stamp>/` (a render trace's `trace.ndjson` and its `frameNNN_eye<N>.png` images) and
//!   `videos/<stamp>/` (a video recording's `stream.json` and `frames.jsonl` sidecar).
//!
//! Folders are plural, and no file name repeats the kind its folder already names. Each folder is
//! created when its first artifact is written, so a run only materializes the folders it uses. The
//! same layout is described for readers of the output in `docs/mod/sessions.md`.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// The files kept once per session, directly in the session root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionFile {
    /// The tracing log.
    Log,
    /// The crash handler's log, written through a raw handle so it survives a broken tracing path.
    CrashLog,
    /// Take markers made while no video recording runs, one JSON object per line.
    Markers,
}

impl SessionFile {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Log => "session.log",
            Self::CrashLog => "crash.log",
            Self::Markers => "markers.jsonl",
        }
    }
}

/// The artifact kinds that write one file per capture, `<folder>/<stamp>[_<qualifier>].<ext>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureFile {
    /// A profiler capture in Chrome trace-event JSON.
    Profile,
    /// An F12 screenshot; its JSON sidecar sits beside it with the same stem.
    Screenshot,
    /// A grapple telemetry capture.
    GrappleLog,
    /// A tracking recording, or a save of the lookback (qualified `lookback`).
    Recording,
    /// A scenario's summary, qualified with the scenario file's stem.
    ScenarioSummary,
    /// The vertex-shader name census.
    ShaderDump,
}

impl CaptureFile {
    pub const fn folder(self) -> &'static str {
        match self {
            Self::Profile => "profiles",
            Self::Screenshot => "screenshots",
            Self::GrappleLog => "grapple-logs",
            Self::Recording => "recordings",
            Self::ScenarioSummary => "scenarios",
            Self::ShaderDump => "shader-dumps",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Profile => "json",
            Self::Screenshot => "png",
            Self::GrappleLog => "csv",
            Self::Recording => tracking_format::EXTENSION,
            Self::ScenarioSummary => "toml",
            Self::ShaderDump => "txt",
        }
    }

    /// The capture's file name, `<stamp>[_<qualifier>].<ext>`. Fails when the qualifier is not a
    /// plain name: empty, or holding a path separator or a parent-directory component.
    pub fn file_name(self, stamp: &str, qualifier: Option<&str>) -> Result<String, SessionError> {
        let ext = self.extension();
        match qualifier {
            None => Ok(format!("{stamp}.{ext}")),
            Some(q) if is_plain_name(q) => Ok(format!("{stamp}_{q}.{ext}")),
            Some(q) => Err(SessionError::InvalidQualifier(q.to_owned())),
        }
    }
}

/// The artifact kinds that write several files per capture into `<folder>/<stamp>/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureFolder {
    /// A render trace: `trace.ndjson` and the per-eye frame images.
    Trace,
    /// A video recording's sidecar: `stream.json` and `frames.jsonl`.
    Video,
}

impl CaptureFolder {
    pub const fn folder(self) -> &'static str {
        match self {
            Self::Trace => "traces",
            Self::Video => "videos",
        }
    }

    /// The capture's folder relative to the session root, `<folder>/<stamp>`, for display.
    pub fn relative(self, stamp: &str) -> PathBuf {
        Path::new(self.folder()).join(stamp)
    }
}

/// Why a session path could not be produced.
#[derive(Debug)]
pub enum SessionError {
    /// The payload DLL's path could not be resolved, so there is no session root.
    Unavailable,
    /// A file name qualifier was not a plain name.
    InvalidQualifier(String),
    /// A folder could not be created.
    Create {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session: ")?;
        match self {
            Self::Unavailable => write!(
                f,
                "the session folder is unavailable (the payload DLL's path is unknown)"
            ),
            Self::InvalidQualifier(q) => write!(f, "{q:?} is not a plain file name qualifier"),
            Self::Create { path, source } => {
                write!(f, "could not create {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Create { source, .. } => Some(source),
            Self::Unavailable | Self::InvalidQualifier(_) => None,
        }
    }
}

/// Resolve this session's output root eagerly so the session stamp is the startup time rather than
/// the first artifact write. Call once early in startup, before logging installs. Does not create the
/// folder; that happens when the first artifact is written.
pub fn init() {
    let _ = root();
}

/// A fresh local wall-clock stamp, `YYYY-MM-DD_HH-MM-SS_mmm`: the one format every session artifact
/// is named with. The session root takes one at startup; every capture takes a fresh one. The
/// millisecond suffix keeps two stamps taken within the same second -- notably an uninject/reinject
/// cycle during development -- from resolving to the same path.
pub fn stamp() -> String {
    jiff::Zoned::now()
        .strftime("%Y-%m-%d_%H-%M-%S_%3f")
        .to_string()
}

/// The path of a once-per-session file in the root, creating the root if missing.
pub fn file(kind: SessionFile) -> Result<PathBuf, SessionError> {
    Ok(created(root_dir()?)?.join(kind.name()))
}

/// The path of one capture's file, `<folder>/<stamp>[_<qualifier>].<ext>`, creating its folder if
/// missing. The caller passes the capture's [`stamp`] so it can record the same one elsewhere.
pub fn capture_file(
    kind: CaptureFile,
    stamp: &str,
    qualifier: Option<&str>,
) -> Result<PathBuf, SessionError> {
    let name = kind.file_name(stamp, qualifier)?;
    Ok(created(root_dir()?.join(kind.folder()))?.join(name))
}

/// One capture's folder, `<folder>/<stamp>/`, created if missing.
pub fn capture_folder(kind: CaptureFolder, stamp: &str) -> Result<PathBuf, SessionError> {
    created(root_dir()?.join(kind.relative(stamp)))
}

fn root_dir() -> Result<PathBuf, SessionError> {
    root().cloned().ok_or(SessionError::Unavailable)
}

/// This session's output root: `sessions/<session stamp>/` beside the payload DLL, or `None` if the
/// DLL path could not be resolved. Resolved once on first access and cached, so the stamp is the
/// run's start time regardless of which artifact writes first. Computing the path does not create it.
fn root() -> Option<&'static PathBuf> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = crate::module::get_path()?
            .parent()?
            .join("sessions")
            .join(stamp());
        Some(dir)
    })
    .as_ref()
}

fn created(dir: PathBuf) -> Result<PathBuf, SessionError> {
    match std::fs::create_dir_all(&dir) {
        Ok(()) => Ok(dir),
        Err(source) => Err(SessionError::Create { path: dir, source }),
    }
}

/// Whether `name` is a single plain path component: non-empty, no separator, not `.` or `..`.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\'])
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use pretty_assertions::assert_eq;

    use super::*;

    const ALL_FILES: [CaptureFile; 6] = [
        CaptureFile::Profile,
        CaptureFile::Screenshot,
        CaptureFile::GrappleLog,
        CaptureFile::Recording,
        CaptureFile::ScenarioSummary,
        CaptureFile::ShaderDump,
    ];
    const ALL_FOLDERS: [CaptureFolder; 2] = [CaptureFolder::Trace, CaptureFolder::Video];

    /// The format string is parsed at run time, so a directive the version of `jiff` in use does not
    /// accept would surface as a malformed session path on the startup path rather than as a build
    /// failure. Pin the shape instead: `YYYY-MM-DD_HH-MM-SS_mmm`, all digits, path-safe.
    #[test]
    fn stamp_is_second_and_millisecond_precise() {
        let stamp = stamp();
        let (date, rest) = stamp
            .split_once('_')
            .expect("date and time are `_`-separated");
        let (time, millis) = rest
            .split_once('_')
            .expect("a millisecond suffix is present");

        assert_eq!(date.len(), 10, "date is `YYYY-MM-DD`: {stamp}");
        assert_eq!(time.len(), 8, "time is `HH-MM-SS`: {stamp}");
        assert_eq!(millis.len(), 3, "milliseconds are three digits: {stamp}");
        assert!(millis.bytes().all(|b| b.is_ascii_digit()), "{stamp}");
        assert_eq!(date.matches('-').count(), 2, "{stamp}");
        assert_eq!(time.matches('-').count(), 2, "{stamp}");
    }

    #[test]
    fn capture_file_names_are_the_stamp_and_an_optional_qualifier() {
        let stamp = "2026-01-02_03-04-05_006";
        assert_eq!(
            CaptureFile::Profile.file_name(stamp, None).unwrap(),
            "2026-01-02_03-04-05_006.json"
        );
        assert_eq!(
            CaptureFile::Recording
                .file_name(stamp, Some("lookback"))
                .unwrap(),
            "2026-01-02_03-04-05_006_lookback.jc3trk"
        );
    }

    #[test]
    fn qualifiers_cannot_escape_the_folder() {
        for bad in ["", ".", "..", "a/b", "..\\escape"] {
            assert!(
                matches!(
                    CaptureFile::ScenarioSummary.file_name("s", Some(bad)),
                    Err(SessionError::InvalidQualifier(_))
                ),
                "{bad:?}"
            );
        }
        assert!(
            CaptureFile::ScenarioSummary
                .file_name("s", Some("grip.v2"))
                .is_ok()
        );
    }

    /// Every kind has a folder of its own, a plain plural name that no root file shares.
    #[test]
    fn every_kind_has_a_distinct_plural_folder() {
        let folders: Vec<&str> = ALL_FILES
            .iter()
            .map(|k| k.folder())
            .chain(ALL_FOLDERS.iter().map(|k| k.folder()))
            .collect();
        let unique: HashSet<&str> = folders.iter().copied().collect();
        assert_eq!(unique.len(), folders.len(), "{folders:?}");
        for folder in folders {
            assert!(is_plain_name(folder), "{folder}");
            assert!(folder.ends_with('s'), "{folder} is plural");
            for file in [
                SessionFile::Log,
                SessionFile::CrashLog,
                SessionFile::Markers,
            ] {
                assert_ne!(folder, file.name());
            }
        }
    }

    #[test]
    fn capture_folders_are_stamped() {
        assert_eq!(
            CaptureFolder::Video.relative("s"),
            Path::new("videos").join("s")
        );
    }
}
