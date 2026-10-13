//! Opt-in, bounded controller status history for diagnosing tracking and hand-control problems.
//!
//! The game-thread hook calls [`record`] after the engine's render update, so the late hand
//! snapshot and the frame's sub-tick fraction it is evaluated at are the ones that frame rendered
//! with. Recording is disabled by default, sampled at [`SAMPLE_RATE_HZ`] on average, and retained
//! in a bounded ring. Captures copy that ring before serializing or writing it, so file I/O never
//! runs while the history lock is held, and are written to the session's `controller_diagnostics`
//! folder. A sample combines independently read subsystem snapshots; it does not claim an atomic
//! cross-subsystem instant.

use std::{
    collections::VecDeque,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::Serialize;

use crate::session;

mod sample;

pub use sample::ControllerDiagnostic;

/// The maximum number of samples retained by the diagnostic history.
pub const MAX_SAMPLES: usize = 120;
/// The average sampling rate while recording.
pub const SAMPLE_RATE_HZ: u32 = 30;
/// The interval between scheduled diagnostic samples.
pub const SAMPLE_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / SAMPLE_RATE_HZ as u64);
/// The schema version written by [`capture`].
pub const SCHEMA_VERSION: u32 = 2;
/// The UI-facing state of the collector and its most recent capture attempt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureStatus {
    pub enabled: bool,
    pub sample_count: usize,
    pub capture_pending: bool,
    pub last_path: Option<String>,
    pub last_error: Option<String>,
}

/// Enable or disable collection. Enabling starts a fresh bounded history.
pub fn set_enabled(enabled: bool) {
    let mut collector = COLLECTOR.lock();
    if collector.enabled == enabled {
        return;
    }
    collector.enabled = enabled;
    if enabled {
        collector.started_at = None;
        collector.next_sample_at = None;
        collector.samples.clear();
    }
}

/// Return a cheap UI status without exposing the history lock.
pub fn status() -> CaptureStatus {
    let _registration = WRITER_REGISTRATION.lock();
    reap_finished_writer();
    let collector = COLLECTOR.lock();
    CaptureStatus {
        enabled: collector.enabled,
        sample_count: collector.samples.len(),
        capture_pending: collector.capture_pending || WRITER_HANDLE.lock().is_some(),
        last_path: collector.last_path.clone(),
        last_error: collector.last_error.clone(),
    }
}

/// Record the current controller state when collection is enabled and a sample is due.
///
/// Recording spawns nothing, so it does not take the writer registration lock; a disabled or
/// not-yet-due call costs one uncontended lock.
pub fn record() {
    if SHUTTING_DOWN.load(Ordering::Acquire) || crate::is_shutting_down() {
        return;
    }
    let now = Instant::now();
    {
        let collector = COLLECTOR.lock();
        if !collector.enabled || !collector.due(now) {
            return;
        }
    }
    let mut sample = sample::snapshot();
    let mut collector = COLLECTOR.lock();
    if !collector.enabled {
        return;
    }
    if collector.started_at.is_none() {
        collector.started_at = Some(now);
    }
    let started_at = collector.started_at.unwrap_or(now);
    sample.elapsed_ms = now
        .saturating_duration_since(started_at)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    collector.push(now, sample);
}

/// Queue the current history for background serialization and writing.
///
/// Only the bounded sample copy and queue state are touched on the caller's thread. A second capture
/// is rejected while the one-at-a-time writer owns the pending job.
pub fn capture() -> Result<(), CaptureError> {
    let _registration = WRITER_REGISTRATION.lock();
    reap_finished_writer();
    if SHUTTING_DOWN.load(Ordering::Acquire) || crate::is_shutting_down() {
        let error = CaptureError::ShuttingDown;
        let mut collector = COLLECTOR.lock();
        collector.last_error = Some(error.to_string());
        return Err(error);
    }
    if WRITER_HANDLE.lock().is_some() {
        let error = CaptureError::Busy;
        let mut collector = COLLECTOR.lock();
        collector.last_error = Some(error.to_string());
        return Err(error);
    }
    let samples = {
        let mut collector = COLLECTOR.lock();
        if collector.samples.is_empty() {
            let error = CaptureError::Empty;
            collector.last_error = Some(error.to_string());
            return Err(error);
        }
        if collector.capture_pending {
            let error = CaptureError::Busy;
            collector.last_error = Some(error.to_string());
            return Err(error);
        }
        collector.capture_pending = true;
        collector.last_error = None;
        collector.samples.iter().cloned().collect::<Vec<_>>()
    };
    let handle = match thread::Builder::new()
        .name("jc3vr-controller-diagnostics".to_owned())
        .spawn(move || finish_capture(samples))
    {
        Ok(handle) => handle,
        Err(source) => {
            let error = CaptureError::Spawn(source);
            let mut collector = COLLECTOR.lock();
            collector.capture_pending = false;
            collector.last_error = Some(error.to_string());
            return Err(error);
        }
    };
    *WRITER_HANDLE.lock() = Some(handle);
    Ok(())
}

fn finish_capture(samples: Vec<ControllerDiagnostic>) {
    let result = write_capture(&samples);
    let mut collector = COLLECTOR.lock();
    collector.capture_pending = false;
    match result {
        Ok(path) => {
            collector.last_path = Some(path.to_string_lossy().into_owned());
            collector.last_error = None;
            tracing::info!(target: "controllers", path = %path.display(), samples = samples.len(), "controller diagnostics captured");
        }
        Err(error) => {
            collector.last_error = Some(error.to_string());
            tracing::warn!(target: "controllers", "{error}");
        }
    }
}

fn write_capture(samples: &[ControllerDiagnostic]) -> Result<PathBuf, CaptureError> {
    let document = CaptureDocument {
        schema_version: SCHEMA_VERSION,
        max_samples: MAX_SAMPLES,
        sample_rate_hz: SAMPLE_RATE_HZ,
        samples: samples.to_vec(),
    };
    let json = serde_json::to_string_pretty(&document).map_err(CaptureError::Serialize)?;
    let dir = session::subdir(CAPTURE_DIR)
        .ok_or(CaptureError::SessionUnavailable)?
        .map_err(CaptureError::Directory)?;
    let path = capture_path(&dir)?;
    write_new(&path, json.as_bytes())?;
    Ok(path)
}

/// Write `contents` to a temporary file beside `path`, then move it into place, so that a failed or
/// interrupted write never leaves a truncated capture under the final name.
fn write_new(path: &Path, contents: &[u8]) -> Result<(), CaptureError> {
    let temporary = path.with_extension("json.tmp");
    let write_error = |source| CaptureError::Write {
        path: temporary.clone(),
        source,
    };
    let result = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(write_error)
        .and_then(|mut file| {
            file.write_all(contents).map_err(write_error)?;
            file.sync_all().map_err(write_error)
        })
        .and_then(|()| {
            fs::rename(&temporary, path).map_err(|source| CaptureError::Write {
                path: path.to_owned(),
                source,
            })
        });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// An error from resolving, serializing, or writing a controller capture.
#[derive(Debug)]
pub enum CaptureError {
    Empty,
    Busy,
    ShuttingDown,
    SessionUnavailable,
    Directory(io::Error),
    NamesExhausted(PathBuf),
    Serialize(serde_json::Error),
    Write { path: PathBuf, source: io::Error },
    Spawn(io::Error),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::Empty => {
                write!(f, "controller diagnostics: there are no samples to capture")
            }
            CaptureError::Busy => write!(
                f,
                "controller diagnostics: a capture is already being written"
            ),
            CaptureError::ShuttingDown => {
                write!(
                    f,
                    "controller diagnostics: capture is unavailable during shutdown"
                )
            }
            CaptureError::SessionUnavailable => write!(
                f,
                "controller diagnostics: the session folder is unavailable"
            ),
            CaptureError::Directory(source) => write!(
                f,
                "controller diagnostics: could not create the capture folder: {source}"
            ),
            CaptureError::NamesExhausted(dir) => write!(
                f,
                "controller diagnostics: no free capture name in {}",
                dir.display()
            ),
            CaptureError::Serialize(error) => {
                write!(
                    f,
                    "controller diagnostics: serializing the capture failed: {error}"
                )
            }
            CaptureError::Write { path, source } => write!(
                f,
                "controller diagnostics: writing {} failed: {source}",
                path.display()
            ),
            CaptureError::Spawn(source) => {
                write!(
                    f,
                    "controller diagnostics: starting the capture writer failed: {source}"
                )
            }
        }
    }
}

impl std::error::Error for CaptureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CaptureError::Serialize(error) => Some(error),
            CaptureError::Write { source, .. }
            | CaptureError::Spawn(source)
            | CaptureError::Directory(source) => Some(source),
            CaptureError::Empty
            | CaptureError::Busy
            | CaptureError::ShuttingDown
            | CaptureError::SessionUnavailable
            | CaptureError::NamesExhausted(_) => None,
        }
    }
}

#[derive(Debug, Serialize)]
struct CaptureDocument {
    schema_version: u32,
    max_samples: usize,
    sample_rate_hz: u32,
    samples: Vec<ControllerDiagnostic>,
}

struct Collector {
    enabled: bool,
    started_at: Option<Instant>,
    /// When the next sample is scheduled. Scheduling from the previous due time rather than from
    /// the previous sample keeps the average rate at [`SAMPLE_RATE_HZ`] when the frame period does
    /// not divide the interval.
    next_sample_at: Option<Instant>,
    samples: VecDeque<ControllerDiagnostic>,
    capture_pending: bool,
    last_path: Option<String>,
    last_error: Option<String>,
}

impl Collector {
    fn due(&self, now: Instant) -> bool {
        self.next_sample_at.is_none_or(|next| now >= next)
    }

    fn push(&mut self, now: Instant, sample: ControllerDiagnostic) {
        if !self.due(now) {
            return;
        }
        // After a stall longer than an interval, restart the schedule instead of catching up.
        self.next_sample_at = Some(match self.next_sample_at {
            Some(next) if now.saturating_duration_since(next) < SAMPLE_INTERVAL => {
                next + SAMPLE_INTERVAL
            }
            _ => now + SAMPLE_INTERVAL,
        });
        if self.samples.len() == MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }
}

fn reap_finished_writer() {
    let finished = WRITER_HANDLE
        .lock()
        .as_ref()
        .is_some_and(JoinHandle::is_finished);
    if !finished {
        return;
    }
    let handle = WRITER_HANDLE
        .lock()
        .take()
        .expect("finished diagnostics writer disappeared");
    if handle.join().is_err() {
        let mut collector = COLLECTOR.lock();
        collector.last_error =
            Some("controller diagnostics: the writer thread panicked".to_owned());
    }
}

/// Stop accepting records and captures, then wait for the diagnostics writer to finish.
///
/// A `false` result means the writer remains retained and the payload must stay mapped until a later
/// shutdown attempt succeeds. The registration lock closes the race between spawning a writer and
/// beginning this wait.
#[must_use = "a live diagnostics writer means the payload must stay mapped"]
pub fn shutdown() -> bool {
    let registration = WRITER_REGISTRATION.lock();
    SHUTTING_DOWN.store(true, Ordering::Release);
    let handle = WRITER_HANDLE.lock().take();
    drop(registration);
    let Some(handle) = handle else {
        return true;
    };
    for poll in 0..SHUTDOWN_POLLS {
        if handle.is_finished() {
            if handle.join().is_err() {
                tracing::error!(target: "controllers", "diagnostics writer thread panicked");
            }
            return true;
        }
        thread::sleep(Duration::from_millis(10));
        if poll > 0 && poll % 100 == 0 {
            tracing::debug!(target: "controllers", "diagnostics writer still running, {} s elapsed", poll / 100);
        }
    }
    tracing::error!(
        target: "controllers",
        "diagnostics writer did not finish within {} s; leaving the payload mapped rather than unmapping under a live thread",
        SHUTDOWN_POLLS / 100,
    );
    *WRITER_HANDLE.lock() = Some(handle);
    false
}

/// How long [`shutdown`] waits for the writer, in 10 ms polls.
const SHUTDOWN_POLLS: u32 = 1000;

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
static WRITER_REGISTRATION: Mutex<()> = Mutex::new(());
static WRITER_HANDLE: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

static COLLECTOR: Mutex<Collector> = Mutex::new(Collector {
    enabled: false,
    started_at: None,
    next_sample_at: None,
    samples: VecDeque::new(),
    capture_pending: false,
    last_path: None,
    last_error: None,
});

/// The session subdirectory captures are written to.
const CAPTURE_DIR: &str = "controller_diagnostics";

fn capture_path(dir: &Path) -> Result<PathBuf, CaptureError> {
    let stamp = session::stamp();
    (0..1000)
        .map(|sequence| dir.join(format!("jc3vr-controllers-{stamp}-{sequence}.json")))
        .find(|path| !path.exists())
        .ok_or_else(|| CaptureError::NamesExhausted(dir.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::diagnostics::sample::{
        AimDiagnostic, HandLateDiagnostic, HandTierDiagnostic, HandTrackingDiagnostic,
        IkDiagnostic, LateDiagnostic, ModeDiagnostic, RoomscaleDiagnostic, SchemeDiagnostic,
        TierDiagnostic, TrackingDiagnostic,
    };

    fn sample(elapsed_ms: u64) -> ControllerDiagnostic {
        ControllerDiagnostic {
            elapsed_ms,
            frame_sequence: None,
            scheme: SchemeDiagnostic {
                effective: "controllers",
                selected: "controllers",
            },
            mode: ModeDiagnostic {
                mode: "on_foot",
                action_set: "onfoot",
                traversal: None,
                held: None,
            },
            tracking: TrackingDiagnostic {
                age_ms: None,
                focused: false,
                action_set: "onfoot",
                input_error: None,
                left: HandTrackingDiagnostic::default(),
                right: HandTrackingDiagnostic::default(),
            },
            ik: [IkDiagnostic {
                queued: false,
                skip: None,
                weight: 0.0,
                reach_distance_m: 0.0,
                rotation_deg: 0.0,
                secondary_wrist_off: false,
                secondary_grip_off: false,
                overrode_live_target: false,
            }; 2],
            tiers: TierDiagnostic {
                state: None,
                left: HandTierDiagnostic {
                    assigned: "free",
                    effective: "free",
                    controller_weight: 1.0,
                    anchor_weight: 0.0,
                },
                right: HandTierDiagnostic {
                    assigned: "free",
                    effective: "free",
                    controller_weight: 1.0,
                    anchor_weight: 0.0,
                },
            },
            late: LateDiagnostic {
                hands: [HandLateDiagnostic {
                    reason: "disabled",
                    physical_displacement_m: None,
                    relative_rotation_deg: None,
                    requested_wrist_position: None,
                    reference_wrist_position: None,
                    latest_wrist_position: None,
                    achieved_wrist_position_model: None,
                }; 2],
                weapons_updated: 0,
                palettes_edited: 0,
            },
            aim: AimDiagnostic {
                idle: Some("disabled"),
                left: None,
                right: None,
                grapple: None,
                weapon: None,
                raycast_error: None,
            },
            roomscale: RoomscaleDiagnostic {
                active: false,
                posture: "standing",
                stage_available: false,
                stage_bounds_m: None,
                stage_head_height_m: None,
                eye_drop_m: 0.0,
                root_owner: None,
                gap_m: 0.0,
                residual_m: 0.0,
                pushback_m: 0.0,
                cockpit_shift_m: [0.0; 3],
                requested_chase_velocity_mps: [0.0; 2],
                requested_task_velocity_mps: [0.0; 2],
                crouched: false,
                seated_crouch: false,
                seat: "out",
                probe: None,
                fade_level: 0.0,
            },
        }
    }

    #[test]
    fn history_is_bounded_and_keeps_newest_order() {
        let mut collector = Collector {
            enabled: true,
            started_at: None,
            next_sample_at: None,
            samples: VecDeque::new(),
            capture_pending: false,
            last_path: None,
            last_error: None,
        };
        let start = Instant::now();
        for i in 0..(MAX_SAMPLES + 4) {
            collector.push(
                start + SAMPLE_INTERVAL.saturating_mul(i as u32),
                sample(i as u64),
            );
        }
        assert_eq!(collector.samples.len(), MAX_SAMPLES);
        assert_eq!(collector.samples.front().unwrap().elapsed_ms, 4);
        assert_eq!(
            collector.samples.back().unwrap().elapsed_ms,
            (MAX_SAMPLES + 3) as u64
        );
    }

    #[test]
    fn samples_are_not_taken_faster_than_the_interval() {
        let mut collector = Collector {
            enabled: true,
            started_at: None,
            next_sample_at: None,
            samples: VecDeque::new(),
            capture_pending: false,
            last_path: None,
            last_error: None,
        };
        let start = Instant::now();
        collector.push(start, sample(0));
        collector.push(start + Duration::from_millis(33), sample(33));
        collector.push(start + Duration::from_millis(34), sample(34));
        assert_eq!(collector.samples.len(), 2);
        assert_eq!(collector.samples.back().unwrap().elapsed_ms, 34);
    }

    #[test]
    fn a_90_hz_frame_rate_samples_at_the_configured_rate() {
        let mut collector = Collector {
            enabled: true,
            started_at: None,
            next_sample_at: None,
            samples: VecDeque::new(),
            capture_pending: false,
            last_path: None,
            last_error: None,
        };
        let start = Instant::now();
        let frame = Duration::from_secs(1) / 90;
        for i in 0..90 {
            collector.push(start + frame * i, sample(u64::from(i)));
        }
        assert_eq!(collector.samples.len(), SAMPLE_RATE_HZ as usize);
    }

    #[test]
    fn capture_schema_names_its_version_rate_and_sample_fields() {
        let document = CaptureDocument {
            schema_version: SCHEMA_VERSION,
            max_samples: MAX_SAMPLES,
            sample_rate_hz: SAMPLE_RATE_HZ,
            samples: vec![sample(17)],
        };
        let value = serde_json::to_value(document).unwrap();
        assert_eq!(value["schema_version"], SCHEMA_VERSION);
        assert_eq!(value["max_samples"], MAX_SAMPLES);
        assert_eq!(value["sample_rate_hz"], SAMPLE_RATE_HZ);
        let sample = &value["samples"][0];
        let mut keys: Vec<&str> = sample
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "aim",
                "elapsed_ms",
                "frame_sequence",
                "ik",
                "late",
                "mode",
                "roomscale",
                "scheme",
                "tiers",
                "tracking",
            ]
        );
        assert_eq!(sample["elapsed_ms"], 17);
        let mut late: Vec<&str> = sample["late"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        late.sort_unstable();
        assert_eq!(late, ["hands", "palettes_edited", "weapons_updated"]);
        assert_eq!(sample["late"]["hands"][0]["reason"], "disabled");
    }
}
