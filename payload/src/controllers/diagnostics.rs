//! Opt-in, bounded controller status history for diagnosing tracking and hand-control problems.
//!
//! The game-thread hook calls [`record`] after the render-time late hand snapshot. Recording is
//! disabled by default, sampled at no more than 30 Hz, and retained in a bounded ring. Captures
//! copy that ring before serializing or writing it, so file I/O never runs while the history lock is
//! held. A sample combines independently read subsystem snapshots; it does not claim an atomic
//! cross-subsystem instant.

use std::{
    collections::VecDeque,
    fmt,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::Serialize;

use crate::controllers::{
    aim::{self, AimIdle, HandSource},
    frame::{self, ActionSetKind, Hand},
    hands::{ik, late, tiers},
    mode::{self, AirborneVariant, Mode, OnFootVariant, Traversal},
    roomscale::{self, probe::ProbeResult},
    scheme::{self, Scheme},
};

/// The maximum number of samples retained by the diagnostic history.
pub const MAX_SAMPLES: usize = 120;
/// The minimum interval between diagnostic samples.
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(34);
/// The schema version written by [`capture`].
pub const SCHEMA_VERSION: u32 = 1;

/// A compact, pointer-free snapshot of the controller subsystems.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ControllerDiagnostic {
    /// Milliseconds since recording was enabled.
    pub elapsed_ms: u64,
    /// The published OpenXR frame sequence, when a frame is available.
    pub frame_sequence: Option<u64>,
    pub scheme: SchemeDiagnostic,
    pub mode: ModeDiagnostic,
    pub tracking: TrackingDiagnostic,
    pub ik: [IkDiagnostic; 2],
    pub tiers: TierDiagnostic,
    pub late: LateDiagnostic,
    pub aim: AimDiagnostic,
    pub roomscale: RoomscaleDiagnostic,
}

/// The effective and auto-selected controller schemes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemeDiagnostic {
    pub effective: &'static str,
    pub selected: &'static str,
}

/// The mode detector's result and the action set selected for the next input frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModeDiagnostic {
    pub mode: &'static str,
    pub action_set: &'static str,
    pub traversal: Option<&'static str>,
    pub held: Option<&'static str>,
}

/// Controller frame age, focus, action set, and per-hand tracking flags.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrackingDiagnostic {
    pub age_ms: Option<u64>,
    pub focused: bool,
    pub action_set: &'static str,
    pub input_error: Option<String>,
    pub left: HandTrackingDiagnostic,
    pub right: HandTrackingDiagnostic,
}

/// Validity and measured-tracking state for one hand's grip and aim poses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct HandTrackingDiagnostic {
    pub active: bool,
    pub grip_position_valid: bool,
    pub grip_orientation_valid: bool,
    pub grip_position_tracked: bool,
    pub grip_orientation_tracked: bool,
    pub aim_position_valid: bool,
    pub aim_orientation_valid: bool,
    pub aim_position_tracked: bool,
    pub aim_orientation_tracked: bool,
}

/// One hand's HumanIK result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct IkDiagnostic {
    pub queued: bool,
    pub skip: Option<&'static str>,
    pub weight: f32,
    pub reach_distance_m: f32,
    pub rotation_deg: f32,
    pub secondary_wrist_off: bool,
    pub secondary_grip_off: bool,
    pub overrode_live_target: bool,
}

/// The classified hand tier and its blend state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TierDiagnostic {
    pub state: Option<&'static str>,
    pub left: HandTierDiagnostic,
    pub right: HandTierDiagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HandTierDiagnostic {
    pub assigned: &'static str,
    pub effective: &'static str,
    pub controller_weight: f32,
    pub anchor_weight: f32,
}

/// One hand's render-time late correction status and physical pose samples.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HandLateDiagnostic {
    pub reason: &'static str,
    pub physical_displacement_m: Option<f32>,
    pub relative_rotation_deg: Option<f32>,
    pub requested_wrist_position: Option<[f32; 3]>,
    pub reference_wrist_position: Option<[f32; 3]>,
    pub latest_wrist_position: Option<[f32; 3]>,
    pub achieved_wrist_position_model: Option<[f32; 3]>,
}

/// Render-time late correction status and cumulative counters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LateDiagnostic {
    pub hands: [HandLateDiagnostic; 2],
    /// Legacy UI fields: applied physical displacement and relative rotation.
    pub left_shift_m: Option<f32>,
    pub left_rotation_deg: Option<f32>,
    pub right_shift_m: Option<f32>,
    pub right_rotation_deg: Option<f32>,
    pub weapons_updated: u64,
    pub palettes_edited: u64,
}

/// Aim rays, hit results, weapon sight selection, and structured error information.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AimDiagnostic {
    pub idle: Option<&'static str>,
    pub left: Option<HandAimDiagnostic>,
    pub right: Option<HandAimDiagnostic>,
    pub grapple: Option<GrappleDiagnostic>,
    pub weapon: Option<WeaponDiagnostic>,
    pub raycast_error: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HandAimDiagnostic {
    pub source: &'static str,
    pub gun_index: Option<usize>,
    pub hit: bool,
    pub hit_distance_m: Option<f32>,
    pub hit_object: bool,
    pub target: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GrappleDiagnostic {
    pub hit: bool,
    pub valid: bool,
    pub tetherable: bool,
    pub point: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WeaponDiagnostic {
    pub name_hash: u32,
    pub gun_hands: [Option<&'static str>; 4],
    pub sight_source: &'static str,
    pub sight_offset_m: Option<[f32; 3]>,
    pub sight_angles_deg: [f32; 2],
}

/// Roomscale posture, stage, chase, seat, fade state, and sampled simulation velocities.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoomscaleDiagnostic {
    pub active: bool,
    pub posture: &'static str,
    pub stage_available: bool,
    pub stage_bounds_m: Option<[f32; 2]>,
    pub stage_head_height_m: Option<f32>,
    pub eye_drop_m: f32,
    pub chase_offset_m: [f32; 2],
    pub cockpit_shift_m: [f32; 3],
    pub requested_chase_velocity_mps: [f32; 2],
    pub requested_task_velocity_mps: [f32; 2],
    pub crouched: bool,
    pub seated_crouch: bool,
    pub seat: &'static str,
    pub probe: Option<&'static str>,
    pub fade_level: f32,
}

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
        collector.last_sample_at = None;
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

/// Record the current controller state when collection is enabled and the sampling interval elapsed.
pub fn record() {
    let _registration = WRITER_REGISTRATION.lock();
    if SHUTTING_DOWN.load(Ordering::Acquire) || crate::is_shutting_down() {
        return;
    }
    let now = Instant::now();
    if !COLLECTOR.lock().enabled {
        return;
    }
    let mut sample = snapshot();
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
        sample_interval_ms: SAMPLE_INTERVAL.as_millis() as u64,
        samples: samples.to_vec(),
    };
    let json = serde_json::to_string_pretty(&document).map_err(CaptureError::Serialize)?;
    let path = capture_path().ok_or(CaptureError::ModulePathUnavailable)?;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|source| CaptureError::Write {
            path: path.clone(),
            source,
        })?;
    file.write_all(json.as_bytes())
        .map_err(|source| CaptureError::Write {
            path: path.clone(),
            source,
        })?;
    Ok(path)
}

/// An error from resolving, serializing, or writing a controller capture.
#[derive(Debug)]
pub enum CaptureError {
    Empty,
    Busy,
    ShuttingDown,
    ModulePathUnavailable,
    Serialize(serde_json::Error),
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    Spawn(std::io::Error),
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
            CaptureError::ModulePathUnavailable => {
                write!(f, "controller diagnostics: the payload path is unavailable")
            }
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
            CaptureError::Write { source, .. } => Some(source),
            CaptureError::Spawn(source) => Some(source),
            CaptureError::Empty
            | CaptureError::Busy
            | CaptureError::ShuttingDown
            | CaptureError::ModulePathUnavailable => None,
        }
    }
}

#[derive(Debug, Serialize)]
struct CaptureDocument {
    schema_version: u32,
    max_samples: usize,
    sample_interval_ms: u64,
    samples: Vec<ControllerDiagnostic>,
}

struct Collector {
    enabled: bool,
    started_at: Option<Instant>,
    last_sample_at: Option<Instant>,
    samples: VecDeque<ControllerDiagnostic>,
    capture_pending: bool,
    last_path: Option<String>,
    last_error: Option<String>,
}

impl Collector {
    fn push(&mut self, now: Instant, sample: ControllerDiagnostic) {
        if self
            .last_sample_at
            .is_some_and(|last| now.saturating_duration_since(last) < SAMPLE_INTERVAL)
        {
            return;
        }
        self.last_sample_at = Some(now);
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
    last_sample_at: None,
    samples: VecDeque::new(),
    capture_pending: false,
    last_path: None,
    last_error: None,
});

fn capture_path() -> Option<PathBuf> {
    let parent = crate::module::get_path()?.parent()?.to_owned();
    let stamp = crate::session::stamp();
    for sequence in 0..1000 {
        let path = parent.join(format!("controller_diagnostics_v1-{stamp}-{sequence}.json"));
        if !path.exists() {
            return Some(path);
        }
    }
    None
}

fn snapshot() -> ControllerDiagnostic {
    let detection = mode::current();
    let input = tracking_snapshot();
    let ik_status = ik::status();
    let tier_status = tiers::status();
    let late_status = late::status();
    let aim_status = aim::status();
    let roomscale_status = roomscale::status();
    let scheme = SchemeDiagnostic {
        effective: scheme_label(scheme::current()),
        selected: scheme_label(scheme::selected()),
    };
    ControllerDiagnostic {
        elapsed_ms: 0,
        frame_sequence: input.sequence,
        scheme,
        mode: ModeDiagnostic {
            mode: mode_label(detection.mode),
            action_set: action_set_label(detection.mode.action_set()),
            traversal: detection.traversal.map(traversal_label),
            held: detection.held.map(hold_label),
        },
        tracking: input.tracking,
        ik: Hand::BOTH.map(|hand| ik_diagnostic(ik_status[hand.index()])),
        tiers: tier_diagnostic(tier_status),
        late: late_diagnostic(late_status),
        aim: aim_diagnostic(aim_status),
        roomscale: roomscale_diagnostic(roomscale_status),
    }
}

struct TrackingSnapshot {
    sequence: Option<u64>,
    tracking: TrackingDiagnostic,
}

fn tracking_snapshot() -> TrackingSnapshot {
    let input_error = crate::vr::input::status().error;
    frame::with_latest(|frame| TrackingSnapshot {
        sequence: Some(frame.sequence),
        tracking: TrackingDiagnostic {
            age_ms: Some(frame.age().as_millis().try_into().unwrap_or(u64::MAX)),
            focused: frame.focused,
            action_set: action_set_label(frame.action_set),
            input_error: input_error.clone(),
            left: hand_tracking(frame, Hand::Left),
            right: hand_tracking(frame, Hand::Right),
        },
    })
    .unwrap_or_else(|| TrackingSnapshot {
        sequence: None,
        tracking: TrackingDiagnostic {
            age_ms: None,
            focused: false,
            action_set: action_set_label(crate::vr::input::active_action_set()),
            input_error,
            left: HandTrackingDiagnostic::default(),
            right: HandTrackingDiagnostic::default(),
        },
    })
}

fn hand_tracking(frame: &frame::ControllerFrame, hand: Hand) -> HandTrackingDiagnostic {
    let hand = frame.hand(hand);
    HandTrackingDiagnostic {
        active: hand.active,
        grip_position_valid: hand.grip.position_valid,
        grip_orientation_valid: hand.grip.orientation_valid,
        grip_position_tracked: hand.grip.position_tracked,
        grip_orientation_tracked: hand.grip.orientation_tracked,
        aim_position_valid: hand.aim.position_valid,
        aim_orientation_valid: hand.aim.orientation_valid,
        aim_position_tracked: hand.aim.position_tracked,
        aim_orientation_tracked: hand.aim.orientation_tracked,
    }
}

fn ik_diagnostic(status: ik::HandIkStatus) -> IkDiagnostic {
    IkDiagnostic {
        queued: status.queued,
        skip: status.skip.map(|skip| skip.label()),
        weight: status.weight,
        reach_distance_m: status.reach_distance,
        rotation_deg: status.rotation_deg,
        secondary_wrist_off: status.secondary_wrist_off,
        secondary_grip_off: status.secondary_grip_off,
        overrode_live_target: status.overrode_live_target,
    }
}

fn tier_diagnostic(status: tiers::TierStatus) -> TierDiagnostic {
    TierDiagnostic {
        state: status.state.map(|state| state.label()),
        left: hand_tier_diagnostic(status, Hand::Left),
        right: hand_tier_diagnostic(status, Hand::Right),
    }
}

fn hand_tier_diagnostic(status: tiers::TierStatus, hand: Hand) -> HandTierDiagnostic {
    let index = hand.index();
    HandTierDiagnostic {
        assigned: status.tiers[index].label(),
        effective: status.effective[index].label(),
        controller_weight: status.weights[index],
        anchor_weight: status.anchor_weights[index],
    }
}

fn late_diagnostic(status: late::LateStatus) -> LateDiagnostic {
    let hands = status.hands.map(|hand| HandLateDiagnostic {
        reason: hand.reason.label(),
        physical_displacement_m: hand.physical_displacement_m,
        relative_rotation_deg: hand.relative_rotation_deg,
        requested_wrist_position: hand.requested_wrist_position.map(|p| p.to_array()),
        reference_wrist_position: hand.reference_wrist_position.map(|p| p.to_array()),
        latest_wrist_position: hand.latest_wrist_position.map(|p| p.to_array()),
        achieved_wrist_position_model: hand.achieved_wrist_position_model.map(|p| p.to_array()),
    });
    let (left_shift_m, left_rotation_deg) = status.deltas[Hand::Left.index()]
        .map_or((None, None), |(shift, rotation)| {
            (Some(shift), Some(rotation))
        });
    let (right_shift_m, right_rotation_deg) = status.deltas[Hand::Right.index()]
        .map_or((None, None), |(shift, rotation)| {
            (Some(shift), Some(rotation))
        });
    LateDiagnostic {
        hands,
        left_shift_m,
        left_rotation_deg,
        right_shift_m,
        right_rotation_deg,
        weapons_updated: status.weapons_updated,
        palettes_edited: status.palettes_edited,
    }
}

fn aim_diagnostic(status: aim::AimStatus) -> AimDiagnostic {
    AimDiagnostic {
        idle: status.idle.map(idle_label),
        left: status.hands[Hand::Left.index()]
            .as_ref()
            .map(hand_aim_diagnostic),
        right: status.hands[Hand::Right.index()]
            .as_ref()
            .map(hand_aim_diagnostic),
        grapple: status.grapple.map(|grapple| GrappleDiagnostic {
            hit: grapple.hit,
            valid: grapple.valid,
            tetherable: grapple.tetherable,
            point: grapple.point.to_array(),
        }),
        weapon: status.weapon.map(|weapon| WeaponDiagnostic {
            name_hash: weapon.hash,
            gun_hands: weapon.gun_hands.map(|hand| hand.map(Hand::label)),
            sight_source: sight_source_label(weapon.sight.source),
            sight_offset_m: weapon.sight.sight.map(|sight| sight.offset_m.to_array()),
            sight_angles_deg: weapon.sight.angles_deg().to_array(),
        }),
        raycast_error: status.raycast_error.map(raycast_error_label),
    }
}

fn hand_aim_diagnostic(aim: &aim::HandAim) -> HandAimDiagnostic {
    HandAimDiagnostic {
        source: match aim.source {
            HandSource::AimPose => "aim_pose",
            HandSource::Gun { .. } => "gun_sight",
        },
        gun_index: match aim.source {
            HandSource::AimPose => None,
            HandSource::Gun { gun } => Some(gun),
        },
        hit: aim.hit.is_some(),
        hit_distance_m: aim.hit.map(|hit| hit.distance),
        hit_object: aim.hit.is_some_and(|hit| hit.object.is_some()),
        target: aim.target.to_array(),
    }
}

fn roomscale_diagnostic(status: roomscale::RoomscaleStatus) -> RoomscaleDiagnostic {
    let frame = status.frame;
    RoomscaleDiagnostic {
        active: status.active,
        posture: frame.posture.label(),
        stage_available: frame.stage.available,
        stage_bounds_m: frame.stage.bounds.map(|bounds| bounds.to_array()),
        stage_head_height_m: frame.stage.head_height,
        eye_drop_m: frame.eye_drop_m,
        chase_offset_m: frame.chase_offset.to_array(),
        cockpit_shift_m: frame.cockpit_shift.to_array(),
        requested_chase_velocity_mps: status.sim.chase_velocity.to_array(),
        requested_task_velocity_mps: status.sim.task_velocity.to_array(),
        crouched: frame.crouched,
        seated_crouch: frame.seated_crouch,
        seat: frame.seat.label(),
        probe: frame.probe.map(probe_label),
        fade_level: status.fade.level,
    }
}

fn scheme_label(scheme: Scheme) -> &'static str {
    match scheme {
        Scheme::Controllers => "controllers",
        Scheme::Gamepad => "gamepad",
    }
}

fn action_set_label(action_set: ActionSetKind) -> &'static str {
    action_set.name()
}

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::OnFoot(OnFootVariant::Ground) => "on_foot",
        Mode::OnFoot(OnFootVariant::Swimming) => "swimming",
        Mode::Vehicle(_) => "vehicle",
        Mode::Airborne(AirborneVariant::Falling) => "falling",
        Mode::Airborne(AirborneVariant::Wingsuit) => "wingsuit",
        Mode::Airborne(AirborneVariant::Parachute) => "parachute",
        Mode::Ui => "ui",
    }
}

fn traversal_label(traversal: Traversal) -> &'static str {
    match traversal {
        Traversal::PreReel => "pre_reel",
        Traversal::ReelingIn => "reeling_in",
        Traversal::PreHang => "pre_hang",
        Traversal::ReeledIn => "reeled_in",
    }
}

fn hold_label(reason: mode::HoldReason) -> &'static str {
    match reason {
        mode::HoldReason::Teleporting => "teleporting",
        mode::HoldReason::CharacterUnavailable => "character_unavailable",
        mode::HoldReason::Debouncing => "debouncing",
    }
}

fn idle_label(idle: AimIdle) -> &'static str {
    match idle {
        AimIdle::Disabled => "disabled",
        AimIdle::GamepadScheme => "gamepad_scheme",
        AimIdle::NoHands => "no_hands",
    }
}

fn sight_source_label(source: aim::sights::SightSource) -> &'static str {
    match source {
        aim::sights::SightSource::Config => "config",
        aim::sights::SightSource::Table(grade) => grade.label(),
        aim::sights::SightSource::FirePoint => "fire_point",
    }
}

fn raycast_error_label(error: aim::raycast::RaycastError) -> &'static str {
    match error {
        aim::raycast::RaycastError::NoPhysicsSystem => "no_physics_system",
        aim::raycast::RaycastError::InvalidDistance => "invalid_distance",
    }
}

fn probe_label(probe: ProbeResult) -> &'static str {
    match probe {
        ProbeResult::Clear => "clear",
        ProbeResult::Obstructed { .. } => "obstructed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                left_shift_m: None,
                left_rotation_deg: None,
                right_shift_m: None,
                right_rotation_deg: None,
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
                chase_offset_m: [0.0; 2],
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
            last_sample_at: None,
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
            last_sample_at: None,
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
    fn capture_schema_is_versioned_and_pointer_free() {
        let document = CaptureDocument {
            schema_version: SCHEMA_VERSION,
            max_samples: MAX_SAMPLES,
            sample_interval_ms: SAMPLE_INTERVAL.as_millis() as u64,
            samples: vec![sample(17)],
        };
        let value = serde_json::to_value(document).unwrap();
        assert_eq!(value["schema_version"], SCHEMA_VERSION);
        assert_eq!(value["max_samples"], MAX_SAMPLES);
        assert_eq!(value["samples"][0]["elapsed_ms"], 17);
        assert!(value.get("address").is_none());
        assert!(value["samples"][0]["aim"]["weapon"].is_null());
    }
}
