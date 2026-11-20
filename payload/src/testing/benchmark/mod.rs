//! The frame-time benchmark: render the live game for a while in one render mode and record what
//! every frame cost, for the harness's `benchmark` command (`tools/rendering/benchmark/`).
//!
//! A run applies its mode the way a player would (the debug UI's single-pass button, and FSR and
//! the foveation at their shipped settings), holds the camera at a fixed pose or moves it along a gentle path while the
//! clock and the world run on, waits for the shaders and the render size to apply, warms up, and
//! then records each frame: the frame interval and the always-on frame breakdown's per-thread and
//! per-slot times (`crate::frame_timing`), the engine's draw calls and the stereo draw path's
//! submissions, the draw detours' own cost when asked, and the GPU time of each dispatch from
//! timestamp queries (`crate::profiler::gpu`). The acknowledgement carries every frame, so the
//! statistics are computed off line. See `docs/mod/testing/benchmark.md`.
//!
//! Threading: [`request`] and [`tick`] run on the game thread, from the request poll and the top of
//! the game's update. The GPU records arrive from the draw thread through [`record_gpu_dispatch`],
//! and the draw-thread naming through [`note_draw_thread`].

pub(crate) mod detour_timing;
mod request;
mod sample;

use std::{
    cell::Cell,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Context as _;
use parking_lot::Mutex;
use serde::Serialize;
use windows::{
    Win32::System::Threading::{GetCurrentThread, SetThreadDescription},
    core::HSTRING,
};

pub(crate) use crate::testing::benchmark::request::{
    BenchmarkError, BenchmarkRequest, BenchmarkTable,
};
use crate::{
    config::{CONFIG, Config},
    frame_timing::{self, Slot},
    fsr::FsrConfig,
    hooks::{
        draw_count,
        graphics_engine::{post::post_effects::vr_forces_blurs_off, shader},
    },
    screenshot::harness::draw_census,
    stereo::{config::set_single_pass_feature, single_pass, switches::ShaderBuild},
    testing::{
        benchmark::{
            request::{BenchmarkMode, CameraPath},
            sample::{COLUMN_COUNT, COLUMNS, Reading, frame_row},
        },
        visual::{OrientationOffset, TranslationOffset, overrides::SinglePassOverrides},
    },
    vr::{
        FoveationConfig, FreezeMode,
        pose_control::{self, PoseValues},
    },
};

/// The GPU seams each dispatch record carries a time for, in the order of
/// `crate::profiler::gpu::GpuSeam::ALL`.
pub(crate) const GPU_SEAM_NAMES: [&str; 6] = [
    "pre_draw",
    "gbuffer",
    "scene",
    "post_effects",
    "post_draw",
    "fsr",
];

/// One resolved GPU dispatch, from the profiler's timestamp read-back.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GpuDispatchSample {
    /// The dispatch's eye (0 or 1) or 2 for a far-field dispatch.
    pub(crate) lane: u8,
    /// The GPU's execution window for the dispatch, idle inside it included.
    pub(crate) span_ns: i64,
    /// The GPU's idle gap since the previous dispatch ended, when measurable.
    pub(crate) idle_before_ns: Option<i64>,
    /// The wall time the draw thread spent recording the dispatch.
    pub(crate) submit_ns: i64,
    /// Each seam's GPU time, by [`GPU_SEAM_NAMES`], when it ran.
    pub(crate) seams_ns: [Option<i64>; GPU_SEAM_NAMES.len()],
}

/// Queue a benchmark run. One runs at a time; a request while another is queued or running is
/// refused with a failed acknowledgement.
pub(crate) fn request(request: BenchmarkRequest) {
    let mut state = STATE.lock();
    if state.pending.is_some() || state.run.is_some() {
        drop(state);
        publish_failure(
            &request,
            "benchmark: another run is queued or running".to_owned(),
        );
        return;
    }
    state.pending = Some(request);
}

/// Advance the queued or running benchmark by one frame. Game thread, once per frame, from the top
/// of the game's update.
pub(crate) fn tick() {
    let now = Instant::now();
    let mut state = STATE.lock();
    if state.run.is_none()
        && let Some(request) = state.pending.take()
    {
        match start(&mut state, request.clone(), now) {
            Ok(run) => state.run = Some(run),
            Err(error) => {
                drop(state);
                publish_failure(&request, error);
                return;
            }
        }
    }
    let Some(run) = state.run.as_mut() else {
        return;
    };
    let Some(outcome) = run.advance(now) else {
        return;
    };
    let run = state.run.take().expect("advanced above");
    disarm();
    let restore = run.request.restore;
    match outcome {
        Ok(result) => publish(&run.request, &result),
        Err(error) => publish_failure(&run.request, error),
    }
    if restore {
        restore_config(&mut state);
    }
}

/// Whether the GPU layer should bracket dispatches for a run.
pub(crate) fn gpu_timing_armed() -> bool {
    GPU_ARMED.load(Ordering::Relaxed)
}

/// File one resolved GPU dispatch with the recording run, if one records.
pub(crate) fn record_gpu_dispatch(sample: GpuDispatchSample) {
    if !GPU_RECORDING.load(Ordering::Relaxed) {
        return;
    }
    let mut samples = GPU_SAMPLES.lock();
    if samples.len() < MAX_GPU_DISPATCHES {
        samples.push(sample);
    }
}

/// Count a dispatch whose GPU timestamps were dropped unread.
pub(crate) fn note_gpu_dispatch_dropped() {
    GPU_DROPPED.fetch_add(1, Ordering::Relaxed);
}

/// Name the draw task's thread while a run is active, once per thread, so the harness can find its
/// CPU time among the process's threads.
pub(crate) fn note_draw_thread() {
    if !NAME_THREADS.load(Ordering::Relaxed) {
        return;
    }
    DRAW_THREAD_NAMED.with(|named| {
        if !named.replace(true) {
            name_current_thread("jc3-draw-task");
        }
    });
}

/// Stop any run, for eject, putting the configuration back.
pub(crate) fn shutdown() {
    disarm();
    let mut state = STATE.lock();
    state.pending = None;
    state.run = None;
    restore_config(&mut state);
}

struct State {
    pending: Option<BenchmarkRequest>,
    run: Option<Run>,
    /// The configuration before the first run since the last restoration.
    saved: Option<Config>,
}

static STATE: Mutex<State> = Mutex::new(State {
    pending: None,
    run: None,
    saved: None,
});

static GPU_ARMED: AtomicBool = AtomicBool::new(false);
static GPU_RECORDING: AtomicBool = AtomicBool::new(false);
static GPU_SAMPLES: Mutex<Vec<GpuDispatchSample>> = Mutex::new(Vec::new());
static GPU_DROPPED: AtomicU64 = AtomicU64::new(0);
static NAME_THREADS: AtomicBool = AtomicBool::new(false);

thread_local! {
    static DRAW_THREAD_NAMED: Cell<bool> = const { Cell::new(false) };
}

/// The most frames and GPU dispatches a run keeps: a minute at well over a hundred frames a second.
const MAX_FRAMES: usize = 20_000;
const MAX_GPU_DISPATCHES: usize = 60_000;
/// How long a run may wait for its configuration to apply before it fails.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(90);
/// The frames a run renders under its configuration before it may warm up, past the reload.
const MIN_SETTLE_FRAMES: u32 = 10;

struct Run {
    request: BenchmarkRequest,
    phase: Phase,
    began: Instant,
    /// Whether the run asked for a shader reload, which must complete before it settles.
    reloaded: bool,
    settle_frames: u32,
    settled_after: Duration,
    /// When the camera path's time starts: the warm-up's beginning.
    path_origin: Option<Instant>,
    frames: Vec<[u64; COLUMN_COUNT]>,
}

enum Phase {
    Settling,
    WarmingUp { until: Instant },
    Recording(Box<Recording>),
}

/// A recording run's state: the readings it differences, boxed since they are large.
struct Recording {
    started: Instant,
    epoch_start: f64,
    first: Reading,
    previous: Reading,
    previous_at: Instant,
    dropped_before: u64,
}

/// What a completed run publishes besides its request.
struct RunResult {
    frames: Vec<[u64; COLUMN_COUNT]>,
    gpu: Vec<GpuDispatchSample>,
    gpu_dropped: u64,
    slots: Vec<SlotTotal>,
    environment: Environment,
    timing: RunTiming,
}

fn start(state: &mut State, request: BenchmarkRequest, now: Instant) -> Result<Run, String> {
    if !crate::hooks::in_gameplay() || !crate::vr::is_running_published() {
        return Err("benchmark: the game is not in gameplay with a VR session running".to_owned());
    }
    let single_pass_on = request.mode == BenchmarkMode::SinglePass;
    if single_pass_on && single_pass::probe_if_needed() != single_pass::Capability::Supported {
        return Err(
            "benchmark: the device does not support single-pass viewport routing".to_owned(),
        );
    }
    {
        let mut config = CONFIG.lock();
        let base = state.saved.get_or_insert_with(|| config.clone()).clone();
        let mut next = base;
        // FSR and the foveation as a player runs them, whatever the session configured.
        next.fsr = FsrConfig::new();
        next.foveation = FoveationConfig::new();
        set_single_pass_feature(&mut next, single_pass_on);
        request.switches.apply(&mut next.stereo.single_pass);
        if let Some(scale) = request.resolution_scale {
            next.vr.resolution_scale = scale;
        }
        if let Some(overlap) = request.overlap_render_update {
            next.vr.overlap_render_update = overlap;
        }
        if let Some(fsr) = request.fsr {
            next.fsr.enabled = fsr;
        }
        if let Some(hybrid) = request.fsr_hybrid_velocity {
            next.fsr.hybrid_velocity = hybrid;
        }
        next.vr.freeze_mode = FreezeMode::FullCamera;
        *config = next;
    }
    if request.recapture_camera {
        pose_control::clear();
    }
    if request.install_draw_detours {
        single_pass::ensure_viewport_detours();
    }
    let reloaded = shader::built_with() != Some(ShaderBuild::current());
    if reloaded {
        shader::request_reload();
    }
    NAME_THREADS.store(true, Ordering::Relaxed);
    tracing::info!(
        label = request.label,
        mode = ?request.mode,
        reloaded,
        "benchmark: run started"
    );
    Ok(Run {
        request,
        phase: Phase::Settling,
        began: now,
        reloaded,
        settle_frames: 0,
        settled_after: Duration::ZERO,
        path_origin: None,
        frames: Vec::new(),
    })
}

impl Run {
    /// Advance by the frame that just ended; `Some` once the run is over.
    fn advance(&mut self, now: Instant) -> Option<Result<RunResult, String>> {
        self.drive_camera(now);
        match &mut self.phase {
            Phase::Settling => {
                self.settle_frames += 1;
                if self.settled() {
                    self.settled_after = now - self.began;
                    self.path_origin = Some(now);
                    crate::fsr::request_history_reset();
                    GPU_ARMED.store(self.request.gpu, Ordering::Relaxed);
                    self.phase = Phase::WarmingUp {
                        until: now + Duration::from_secs_f64(self.request.warmup_seconds),
                    };
                } else if now - self.began > SETTLE_TIMEOUT {
                    return Some(Err(format!(
                        "benchmark: the configuration did not apply within {} seconds ({})",
                        SETTLE_TIMEOUT.as_secs(),
                        self.settle_state()
                    )));
                }
                None
            }
            Phase::WarmingUp { until } => {
                if now < *until {
                    return None;
                }
                GPU_SAMPLES.lock().clear();
                GPU_RECORDING.store(self.request.gpu, Ordering::Relaxed);
                detour_timing::set_armed(self.request.detour_timing);
                let reading = read();
                self.phase = Phase::Recording(Box::new(Recording {
                    started: now,
                    epoch_start: epoch_seconds(),
                    first: reading.clone(),
                    previous: reading,
                    previous_at: now,
                    dropped_before: GPU_DROPPED.load(Ordering::Relaxed),
                }));
                None
            }
            Phase::Recording(recording) => {
                let Recording {
                    started,
                    epoch_start,
                    first,
                    previous,
                    previous_at,
                    dropped_before,
                } = recording.as_mut();
                let reading = read();
                let frame_ns = duration_ns(now - *previous_at);
                let (_, sim_ticks) = frame_timing::last_frame();
                if self.frames.len() < MAX_FRAMES {
                    self.frames
                        .push(frame_row(previous, &reading, frame_ns, sim_ticks));
                }
                *previous = reading;
                *previous_at = now;
                let elapsed = now - *started;
                if elapsed.as_secs_f64() < self.request.record_seconds {
                    return None;
                }
                GPU_RECORDING.store(false, Ordering::Relaxed);
                detour_timing::set_armed(false);
                let slots = slot_totals(first, previous);
                let result = RunResult {
                    frames: std::mem::take(&mut self.frames),
                    gpu: std::mem::take(&mut *GPU_SAMPLES.lock()),
                    gpu_dropped: GPU_DROPPED.load(Ordering::Relaxed) - *dropped_before,
                    slots,
                    environment: Environment::current(),
                    timing: RunTiming {
                        settle_seconds: self.settled_after.as_secs_f64(),
                        settle_frames: self.settle_frames,
                        reloaded: self.reloaded,
                        record_seconds: elapsed.as_secs_f64(),
                        epoch_start: *epoch_start,
                        epoch_end: epoch_seconds(),
                    },
                };
                Some(Ok(result))
            }
        }
    }

    /// Whether the configuration has applied: the shaders re-created under it if the run asked for
    /// a reload, the single-pass state as asked, the render size taken, and the camera held.
    fn settled(&self) -> bool {
        let single_pass_on = self.request.mode == BenchmarkMode::SinglePass;
        let shaders = !self.reloaded || shader::built_with() == Some(ShaderBuild::current());
        let sps = single_pass::active() == single_pass_on
            && single_pass::double_wide_active() == single_pass_on;
        let resolution = crate::vr::resolution_status();
        let sized = resolution.target.is_none() || resolution.engine == resolution.target;
        let held = pose_control::current_full_camera().is_some();
        self.settle_frames >= MIN_SETTLE_FRAMES && shaders && sps && sized && held
    }

    fn settle_state(&self) -> String {
        let resolution = crate::vr::resolution_status();
        format!(
            "shaders current {}, single-pass active {}, double-wide {}, engine size {:?}, target \
             size {:?}, camera held {}",
            shader::built_with() == Some(ShaderBuild::current()),
            single_pass::active(),
            single_pass::double_wide_active(),
            resolution.engine,
            resolution.target,
            pose_control::current_full_camera().is_some(),
        )
    }

    /// Move the held camera along the run's path, from the pose captured when the hold engaged.
    fn drive_camera(&self, now: Instant) {
        if self.request.camera.is_still() {
            return;
        }
        let Some(base) = pose_control::base() else {
            return;
        };
        let t = self
            .path_origin
            .map_or(0.0, |origin| (now - origin).as_secs_f32());
        pose_control::set_current(camera_target(base, &self.request.camera, t));
    }
}

/// The pose the path puts the camera at `t` seconds after its origin: the base's yaw swung by a
/// sine of `yaw_deg` amplitude, and its position moved by a sine of `forward_m` amplitude along the
/// base's heading in the horizontal plane.
fn camera_target(base: PoseValues, path: &CameraPath, t: f32) -> PoseValues {
    let phase = (std::f32::consts::TAU * t / path.period_seconds).sin();
    let heading = OrientationOffset {
        yaw_deg: base.yaw_deg,
        pitch_deg: 0.0,
        roll_deg: 0.0,
    }
    .quaternion();
    let forward = TranslationOffset {
        right: 0.0,
        up: 0.0,
        forward: path.forward_m * phase,
    }
    .vector();
    PoseValues {
        position: base.position + heading * forward,
        yaw_deg: base.yaw_deg + path.yaw_deg * phase,
        ..base
    }
}

fn read() -> Reading {
    Reading {
        timing: frame_timing::cumulative(),
        engine: draw_count::engine_call_totals(),
        submissions: draw_census::submission_total(),
        detours: detour_timing::totals(),
        pose_age: crate::vr::pose_age_totals(),
    }
}

fn disarm() {
    GPU_ARMED.store(false, Ordering::Relaxed);
    GPU_RECORDING.store(false, Ordering::Relaxed);
    NAME_THREADS.store(false, Ordering::Relaxed);
    detour_timing::set_armed(false);
}

/// Put back the configuration from before the first run, re-creating the shaders if it differs
/// in what they were built under, and release the held camera.
fn restore_config(state: &mut State) {
    let Some(saved) = state.saved.take() else {
        return;
    };
    *CONFIG.lock() = saved;
    pose_control::clear();
    if shader::built_with() != Some(ShaderBuild::current()) {
        shader::request_reload();
    }
}

#[derive(Serialize)]
struct SlotTotal {
    name: &'static str,
    kind: &'static str,
    self_us: u64,
    calls: u64,
}

fn slot_totals(first: &Reading, last: &Reading) -> Vec<SlotTotal> {
    Slot::ALL
        .iter()
        .map(|&slot| SlotTotal {
            name: slot.name(),
            kind: slot.kind().name(),
            self_us: last.timing.slot_since(&first.timing, slot) / 1000,
            calls: last.timing.calls_since(&first.timing, slot),
        })
        .collect()
}

/// The state a run measured under, read when it finished.
#[derive(Serialize)]
struct Environment {
    build: &'static str,
    profiler_feature: bool,
    single_pass_active: bool,
    double_wide_active: bool,
    com_detours_installed: bool,
    blurs_forced_off: bool,
    engine_size: Option<(u32, u32)>,
    target_size: Option<(u32, u32)>,
    resolution_scale: f32,
    native_resolution: bool,
    fsr_enabled: bool,
    fsr_hybrid_velocity: bool,
    foveation_enabled: bool,
    far_field_enabled: bool,
    overlap_render_update: bool,
    clock_pair_ns: f64,
}

impl Environment {
    fn current() -> Self {
        let resolution = crate::vr::resolution_status();
        let (resolution_scale, native_resolution, fsr, foveation, far_field, overlap) =
            Config::lock_query(|c| {
                (
                    c.vr.resolution_scale,
                    c.vr.native_resolution,
                    c.fsr,
                    c.foveation.enabled,
                    c.far_field.enabled,
                    c.vr.overlap_render_update,
                )
            });
        Self {
            build: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            profiler_feature: cfg!(feature = "profiler"),
            single_pass_active: single_pass::active(),
            double_wide_active: single_pass::double_wide_active(),
            com_detours_installed: single_pass::com_detours_installed(),
            blurs_forced_off: vr_forces_blurs_off(),
            engine_size: resolution.engine,
            target_size: resolution.target,
            resolution_scale,
            native_resolution,
            fsr_enabled: fsr.enabled,
            fsr_hybrid_velocity: fsr.hybrid_velocity,
            foveation_enabled: foveation,
            far_field_enabled: far_field,
            overlap_render_update: overlap,
            clock_pair_ns: detour_timing::clock_pair_ns(10_000),
        }
    }
}

#[derive(Serialize)]
struct RunTiming {
    settle_seconds: f64,
    settle_frames: u32,
    reloaded: bool,
    record_seconds: f64,
    /// The recording's start and end as seconds since the Unix epoch, for matching samples taken
    /// outside the process.
    epoch_start: f64,
    epoch_end: f64,
}

#[derive(Serialize)]
struct RequestEcho<'a> {
    mode: BenchmarkMode,
    warmup_seconds: f64,
    record_seconds: f64,
    resolution_scale: Option<f32>,
    gpu: bool,
    detour_timing: bool,
    install_draw_detours: bool,
    switches: &'a SinglePassOverrides,
    overlap_render_update: Option<bool>,
    fsr: Option<bool>,
    fsr_hybrid_velocity: Option<bool>,
    camera: CameraPath,
}

impl<'a> From<&'a BenchmarkRequest> for RequestEcho<'a> {
    fn from(request: &'a BenchmarkRequest) -> Self {
        Self {
            mode: request.mode,
            warmup_seconds: request.warmup_seconds,
            record_seconds: request.record_seconds,
            resolution_scale: request.resolution_scale,
            gpu: request.gpu,
            detour_timing: request.detour_timing,
            install_draw_detours: request.install_draw_detours,
            switches: &request.switches,
            overlap_render_update: request.overlap_render_update,
            fsr: request.fsr,
            fsr_hybrid_velocity: request.fsr_hybrid_velocity,
            camera: request.camera,
        }
    }
}

#[derive(Serialize)]
struct GpuRecords {
    seams: [&'static str; GPU_SEAM_NAMES.len()],
    columns: Vec<String>,
    /// One row per dispatch, in microseconds; -1 for a value that was not measured.
    dispatches: Vec<Vec<i64>>,
    dropped: u64,
}

#[derive(Serialize)]
struct BenchmarkAck<'a> {
    id: &'a str,
    state: &'static str,
    error: Option<String>,
    label: &'a str,
    request: RequestEcho<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    environment: Option<&'a Environment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timing: Option<&'a RunTiming>,
    columns: &'static [&'static str],
    frames: Vec<&'a [u64]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gpu: Option<GpuRecords>,
    slots: &'a [SlotTotal],
}

fn gpu_records(samples: &[GpuDispatchSample], dropped: u64) -> GpuRecords {
    let us = |ns: i64| (ns + 500).div_euclid(1000);
    let mut columns: Vec<String> = ["lane", "span_us", "idle_before_us", "submit_us"]
        .iter()
        .map(|c| (*c).to_owned())
        .collect();
    columns.extend(GPU_SEAM_NAMES.iter().map(|seam| format!("{seam}_us")));
    let dispatches = samples
        .iter()
        .map(|sample| {
            let mut row = vec![
                i64::from(sample.lane),
                us(sample.span_ns),
                sample.idle_before_ns.map_or(-1, us),
                us(sample.submit_ns),
            ];
            row.extend(sample.seams_ns.iter().map(|ns| ns.map_or(-1, us)));
            row
        })
        .collect();
    GpuRecords {
        seams: GPU_SEAM_NAMES,
        columns,
        dispatches,
        dropped,
    }
}

fn publish(request: &BenchmarkRequest, result: &RunResult) {
    let ack = BenchmarkAck {
        id: request.id.as_str(),
        state: "completed",
        error: None,
        label: &request.label,
        request: request.into(),
        environment: Some(&result.environment),
        timing: Some(&result.timing),
        columns: &COLUMNS,
        frames: result.frames.iter().map(|row| row.as_slice()).collect(),
        gpu: request
            .gpu
            .then(|| gpu_records(&result.gpu, result.gpu_dropped)),
        slots: &result.slots,
    };
    tracing::info!(
        label = request.label,
        frames = result.frames.len(),
        gpu_dispatches = result.gpu.len(),
        "benchmark: run completed"
    );
    write_ack(request, &ack);
}

fn publish_failure(request: &BenchmarkRequest, error: String) {
    tracing::warn!(label = request.label, "{error}");
    let ack = BenchmarkAck {
        id: request.id.as_str(),
        state: "failed",
        error: Some(error),
        label: &request.label,
        request: request.into(),
        environment: None,
        timing: None,
        columns: &COLUMNS,
        frames: Vec::new(),
        gpu: None,
        slots: &[],
    };
    write_ack(request, &ack);
}

fn write_ack(request: &BenchmarkRequest, ack: &BenchmarkAck<'_>) {
    let Some(folder) = crate::config::paths::dir().map(|dir| dir.join("captures")) else {
        tracing::error!("benchmark: no configuration directory for the acknowledgement");
        return;
    };
    let path = folder.join(format!("{}.json", request.id.as_str()));
    if let Err(error) = write_json(&path, ack) {
        tracing::error!("benchmark: {error:#}");
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&parent)
        .with_context(|| format!("could not create {}", parent.display()))?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(value).context("could not encode the acknowledgement")?;
    std::fs::write(&temporary, bytes)
        .with_context(|| format!("could not write {}", temporary.display()))?;
    std::fs::rename(&temporary, path)
        .with_context(|| format!("could not publish {}", path.display()))
}

fn epoch_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn duration_ns(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

/// Name the calling thread, which Wine passes on to the host thread's name.
fn name_current_thread(name: &str) {
    // SAFETY: the pseudo-handle names the calling thread, and the string outlives the call.
    if let Err(error) = unsafe { SetThreadDescription(GetCurrentThread(), &HSTRING::from(name)) } {
        tracing::debug!("benchmark: could not name the thread {name}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    fn base() -> PoseValues {
        PoseValues {
            position: Vec3::new(10.0, 5.0, -3.0),
            yaw_deg: 90.0,
            pitch_deg: -5.0,
            roll_deg: 0.0,
        }
    }

    #[test]
    fn a_still_path_keeps_the_base() {
        let target = camera_target(base(), &CameraPath::STILL, 3.0);
        assert_eq!(target, base());
    }

    #[test]
    fn the_path_peaks_a_quarter_period_in() {
        let path = CameraPath {
            yaw_deg: 10.0,
            forward_m: 4.0,
            period_seconds: 8.0,
        };
        let target = camera_target(base(), &path, 2.0);
        assert!((target.yaw_deg - 100.0).abs() < 1e-4, "{}", target.yaw_deg);
        assert_eq!(target.pitch_deg, -5.0);
        // Forward at a yaw of 90 degrees about +Y is -X: the forward axis is -Z, turned by the yaw.
        let moved = target.position - base().position;
        assert!((moved.length() - 4.0).abs() < 1e-4, "{moved}");
        assert!(
            moved.y.abs() < 1e-5 && (moved.x + 4.0).abs() < 1e-4,
            "{moved}"
        );
        let back = camera_target(base(), &path, 6.0);
        assert!((back.yaw_deg - 80.0).abs() < 1e-4);
    }
}
