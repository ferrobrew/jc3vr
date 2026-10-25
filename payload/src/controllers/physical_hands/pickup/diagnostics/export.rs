//! Opt-in export of one copied grip-acquisition snapshot, without engine reads or history.
//!
//! The versioned JSON projects identities and bounds explicitly. Nested runtime facts use their
//! complete Debug representation so the export does not impose a wire format on shared game types.

#[cfg(test)]
mod tests;

use std::{
    error::Error,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use parking_lot::Mutex;
use serde::Serialize;

use crate::{
    controllers::physical_hands::pickup::diagnostics::{
        CANDIDATE_CAPACITY, Candidate, DriveRecord, HandEvidence, InputRecord, REQUEST_CAPACITY,
        RequestRecord, Status,
        scan::{SCAN_CAPACITY, Scan, ScanEntry},
    },
    session,
};

/// The result of the most recent explicit save, separate from the acquisition evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SaveStatus {
    pub last_path: Option<PathBuf>,
    pub last_error: Option<String>,
}

/// Admission to the single pending slot. A rejected request never replaces the earlier snapshot.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum RequestResult {
    Queued,
    AlreadyPending,
}

/// Copy the save result before rendering it. This lock is never held during formatting or I/O.
pub(crate) fn status() -> SaveStatus {
    SAVE_STATUS.lock().clone()
}

/// The single pending request slot used by the UI and the post-UI service.
pub(crate) fn requests() -> &'static SaveRequests {
    &REQUESTS
}

/// Service at most one request on the game thread, after the UI and EguiState guards are released.
pub(crate) fn service_pending() {
    REQUESTS.service_with(save);
}

/// A save error retains the operation, the affected path, and the underlying cause.
#[derive(Debug)]
pub(crate) enum SaveError {
    SessionUnavailable,
    Directory {
        path: PathBuf,
        source: io::Error,
    },
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },
    Create {
        path: PathBuf,
        source: io::Error,
    },
    NamesExhausted {
        path: PathBuf,
        attempts: usize,
    },
    Write {
        path: PathBuf,
        operation: &'static str,
        source: io::Error,
        cleanup: Option<io::Error>,
    },
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionUnavailable => {
                write!(
                    f,
                    "grip acquisition evidence: the session output root is unavailable"
                )
            }
            Self::Directory { path, source } => write!(
                f,
                "grip acquisition evidence: create directory {}: {source}",
                path.display(),
            ),
            Self::Serialize { path, source } => write!(
                f,
                "grip acquisition evidence: serialize snapshot for {}: {source}",
                path.display(),
            ),
            Self::Create { path, source } => write!(
                f,
                "grip acquisition evidence: create new file {}: {source}",
                path.display(),
            ),
            Self::NamesExhausted { path, attempts } => write!(
                f,
                "grip acquisition evidence: all {attempts} candidate filenames in {} already exist",
                path.display(),
            ),
            Self::Write {
                path,
                operation,
                source,
                cleanup,
            } => {
                write!(
                    f,
                    "grip acquisition evidence: {operation} {}: {source}",
                    path.display()
                )?;
                if let Some(cleanup) = cleanup {
                    write!(f, "; remove incomplete file {}: {cleanup}", path.display())?;
                }
                Ok(())
            }
        }
    }
}

impl Error for SaveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Directory { source, .. }
            | Self::Create { source, .. }
            | Self::Write { source, .. } => Some(source),
            Self::Serialize { source, .. } => Some(source),
            Self::SessionUnavailable | Self::NamesExhausted { .. } => None,
        }
    }
}

const SCHEMA_VERSION: u32 = 1;
const OUTPUT_DIRECTORY: &str = "grip_acquisition";
const NAME_ATTEMPTS: usize = 64;
const IDENTITY_SEMANTICS: &str = "Controller sequence is a publication identity, observation is a per-hand input read ordinal, episode is a per-hand grip episode, and request is a per-hand queue ordinal. Storage order is not event order. No identity is a timestamp or a video-frame correlation.";
const FACTS_FORMAT: &str = "Complete Rust Debug values for nested runtime types, interpreted with build_stamp; not a stable nested schema. Non-finite floats remain Debug text, not JSON null.";
const LIMITATIONS: &str = "One bounded retained snapshot, not a recording or synchronized cross-subsystem sample. Missing provider data is unavailable evidence, not an observed miss. A commanded velocity or successful write is not post-step motion, proof of lift, or proof of another writer. Export time and the session folder are artifact provenance only.";

static REQUESTS: SaveRequests = SaveRequests::new();
static SAVE_STATUS: Mutex<SaveStatus> = Mutex::new(SaveStatus {
    last_path: None,
    last_error: None,
});

/// One copied snapshot awaiting the game thread's post-UI service.
pub(crate) struct SaveRequests {
    pending: Mutex<Option<Status>>,
}

impl SaveRequests {
    pub(crate) const fn new() -> Self {
        Self {
            pending: Mutex::new(None),
        }
    }

    /// Enqueue without formatting or I/O; preserve an existing request on rejection.
    pub(crate) fn request(&self, snapshot: Status) -> RequestResult {
        let mut pending = self.pending.lock();
        if pending.is_some() {
            RequestResult::AlreadyPending
        } else {
            *pending = Some(snapshot);
            RequestResult::Queued
        }
    }

    pub(crate) fn service_with(&self, save: impl FnOnce(Status)) {
        let snapshot = { self.pending.lock().take() };
        if let Some(snapshot) = snapshot {
            save(snapshot);
        }
    }
}

fn save(snapshot: Status) {
    let result = save_session(snapshot);
    let saved = match result {
        Ok(path) => SaveStatus {
            last_path: Some(path),
            last_error: None,
        },
        Err(error) => SaveStatus {
            last_path: None,
            last_error: Some(error.to_string()),
        },
    };
    *SAVE_STATUS.lock() = saved;
}

fn save_session(snapshot: Status) -> Result<PathBuf, SaveError> {
    let root = session::output_root().ok_or(SaveError::SessionUnavailable)?;
    let directory = root.join(OUTPUT_DIRECTORY);
    let document = Document::new(
        snapshot,
        Provenance {
            build_stamp: crate::BUILD_STAMP.to_owned(),
            package_version: env!("CARGO_PKG_VERSION").to_owned(),
            process_id: std::process::id(),
            session_output_root: root.to_string_lossy().into_owned(),
            export_local_timestamp: session::stamp(),
        },
    );
    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
    write_document(&directory, &document, &NEXT_FILE)
}

fn write_document(
    directory: &Path,
    document: &Document,
    next_file: &AtomicU64,
) -> Result<PathBuf, SaveError> {
    let contents = serde_json::to_vec_pretty(document).map_err(|source| SaveError::Serialize {
        path: directory.to_owned(),
        source,
    })?;
    fs::create_dir_all(directory).map_err(|source| SaveError::Directory {
        path: directory.to_owned(),
        source,
    })?;
    for _ in 0..NAME_ATTEMPTS {
        let ordinal = next_file.fetch_add(1, Ordering::Relaxed);
        let path = evidence_path(directory, ordinal);
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(SaveError::Create { path, source }),
        };
        let result = write_contents(&mut file, &contents);
        drop(file);
        if let Err((operation, source)) = result {
            let cleanup = fs::remove_file(&path).err();
            return Err(SaveError::Write {
                path,
                operation,
                source,
                cleanup,
            });
        }
        return Ok(path);
    }
    Err(SaveError::NamesExhausted {
        path: directory.to_owned(),
        attempts: NAME_ATTEMPTS,
    })
}

fn evidence_path(directory: &Path, ordinal: u64) -> PathBuf {
    directory.join(format!("grip-acquisition-{ordinal:03}.json"))
}

fn write_contents(
    file: &mut std::fs::File,
    contents: &[u8],
) -> Result<(), (&'static str, io::Error)> {
    file.write_all(contents)
        .map_err(|source| ("write", source))?;
    file.flush().map_err(|source| ("flush", source))?;
    file.sync_all().map_err(|source| ("sync", source))
}

#[derive(Serialize)]
struct Document {
    schema_version: u32,
    artifact: &'static str,
    provenance: Provenance,
    identity_semantics: &'static str,
    facts_format: &'static str,
    limitations: &'static str,
    candidate_capacity_per_input: usize,
    scan_capacity_per_input: usize,
    request_capacity_per_hand: usize,
    hands: [HandDocument; 2],
}

impl Document {
    fn new(snapshot: Status, provenance: Provenance) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            artifact: "grip-acquisition-evidence",
            provenance,
            identity_semantics: IDENTITY_SEMANTICS,
            facts_format: FACTS_FORMAT,
            limitations: LIMITATIONS,
            candidate_capacity_per_input: CANDIDATE_CAPACITY,
            scan_capacity_per_input: SCAN_CAPACITY,
            request_capacity_per_hand: REQUEST_CAPACITY,
            hands: [
                HandDocument::new("left", snapshot.hands[0]),
                HandDocument::new("right", snapshot.hands[1]),
            ],
        }
    }
}

#[derive(Serialize)]
struct Provenance {
    build_stamp: String,
    package_version: String,
    process_id: u32,
    session_output_root: String,
    export_local_timestamp: String,
}

#[derive(Serialize)]
struct HandDocument {
    hand: &'static str,
    latest: Option<InputDocument>,
    last_press: Option<InputDocument>,
    requests: [Option<RequestDocument>; REQUEST_CAPACITY],
    evicted: u64,
}

impl HandDocument {
    fn new(hand: &'static str, evidence: HandEvidence) -> Self {
        Self {
            hand,
            latest: evidence.latest.map(InputDocument::from),
            last_press: evidence.last_press.map(InputDocument::from),
            requests: evidence
                .requests
                .map(|request| request.map(RequestDocument::from)),
            evicted: evidence.evicted,
        }
    }
}

#[derive(Serialize)]
struct InputDocument {
    sequence: u64,
    observation: u64,
    episode: u64,
    grip: GripDocument,
    latch_debug: String,
    grab_debug: Option<String>,
    chosen: Option<CandidateDocument>,
    candidates: [Option<CandidateDocument>; CANDIDATE_CAPACITY],
    candidates_omitted: usize,
    scan: Option<ScanDocument>,
    consumer_debug: String,
}

impl From<InputRecord> for InputDocument {
    fn from(input: InputRecord) -> Self {
        Self {
            sequence: input.sequence,
            observation: input.observation,
            episode: input.episode,
            grip: GripDocument {
                down: input.grip.down,
                pressed: input.grip.pressed,
                released: input.grip.released,
            },
            latch_debug: format!("{:?}", input.latch),
            grab_debug: input.grab.map(|grab| format!("{grab:#?}")),
            chosen: input.chosen.map(CandidateDocument::from),
            candidates: input
                .candidates
                .map(|candidate| candidate.map(CandidateDocument::from)),
            candidates_omitted: input.candidates_omitted,
            scan: input.scan.map(ScanDocument::from),
            consumer_debug: format!("{:?}", input.consumer),
        }
    }
}

#[derive(Serialize)]
struct GripDocument {
    down: bool,
    pressed: bool,
    released: bool,
}

#[derive(Serialize)]
struct CandidateDocument {
    resolver_index: usize,
    volume_debug: String,
    distance_debug: String,
    tier_debug: String,
    observed_debug: Option<String>,
    lost_debug: Option<String>,
}

impl From<Candidate> for CandidateDocument {
    fn from(candidate: Candidate) -> Self {
        Self {
            resolver_index: candidate.resolved.index,
            volume_debug: format!("{:#?}", candidate.resolved.volume),
            distance_debug: format!("{:?}", candidate.resolved.distance),
            tier_debug: format!("{:?}", candidate.resolved.tier),
            observed_debug: candidate.observed.map(|reach| format!("{reach:#?}")),
            lost_debug: candidate.lost.map(|lost| format!("{lost:#?}")),
        }
    }
}

#[derive(Serialize)]
struct ScanDocument {
    entries: [Option<ScanEntryDocument>; SCAN_CAPACITY],
    omitted: usize,
    ray_ran: bool,
    handles_ran: bool,
    ray_selected_debug: Option<String>,
    handle_selected_debug: Option<String>,
    selected_debug: Option<String>,
}

impl From<Scan> for ScanDocument {
    fn from(scan: Scan) -> Self {
        Self {
            entries: scan.entries.map(|entry| entry.map(ScanEntryDocument::from)),
            omitted: scan.omitted,
            ray_ran: scan.ray_ran,
            handles_ran: scan.handles_ran,
            ray_selected_debug: scan.ray_selected.map(|reach| format!("{reach:#?}")),
            handle_selected_debug: scan.handle_selected.map(|reach| format!("{reach:#?}")),
            selected_debug: scan.selected.map(|reach| format!("{reach:#?}")),
        }
    }
}

#[derive(Serialize)]
struct ScanEntryDocument {
    branch_debug: String,
    object: Option<usize>,
    body: Option<u32>,
    door_debug: Option<String>,
    point_debug: Option<String>,
    distance_debug: Option<String>,
    mass_kg_debug: Option<String>,
    result_debug: String,
}

impl From<ScanEntry> for ScanEntryDocument {
    fn from(entry: ScanEntry) -> Self {
        Self {
            branch_debug: format!("{:?}", entry.branch),
            object: entry.object.map(|object| object.get()),
            body: entry.body,
            door_debug: entry.door.map(|door| format!("{door:#?}")),
            point_debug: entry.point.map(|point| format!("{point:#?}")),
            distance_debug: entry.distance.map(|distance| format!("{distance:?}")),
            mass_kg_debug: entry.mass_kg.map(|mass| format!("{mass:?}")),
            result_debug: format!("{:#?}", entry.result),
        }
    }
}

#[derive(Serialize)]
struct RequestDocument {
    sequence: u64,
    episode: u64,
    request: u64,
    target_debug: String,
    input: InputDocument,
    consumer_debug: String,
    drive: Option<DriveDocument>,
}

impl From<RequestRecord> for RequestDocument {
    fn from(request: RequestRecord) -> Self {
        Self {
            sequence: request.key.sequence,
            episode: request.key.episode,
            request: request.key.request,
            target_debug: format!("{:#?}", request.key.target),
            input: request.input.into(),
            consumer_debug: format!("{:#?}", request.consumer),
            drive: request.drive.map(DriveDocument::from),
        }
    }
}

#[derive(Serialize)]
struct DriveDocument {
    object: usize,
    body: u32,
    before_debug: String,
    commanded_debug: Option<String>,
    result_debug: Option<String>,
    report_debug: Option<String>,
}

impl From<DriveRecord> for DriveDocument {
    fn from(drive: DriveRecord) -> Self {
        Self {
            object: drive.object,
            body: drive.body,
            before_debug: format!("{:#?}", drive.before),
            commanded_debug: drive.commanded.map(|commanded| format!("{commanded:#?}")),
            result_debug: drive.result.map(|result| format!("{result:#?}")),
            report_debug: drive.report.map(|report| format!("{report:#?}")),
        }
    }
}
