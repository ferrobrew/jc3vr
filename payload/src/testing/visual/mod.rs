use std::{
    collections::VecDeque,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use anyhow::Context as _;
use jc3gi::clock::Clock;
use parking_lot::Mutex;
use serde::Serialize;

pub(crate) use crate::stereo::dispatch::RenderMode;
use crate::{
    config::{CONFIG, Config},
    hooks::graphics_engine::shader,
    stereo::{config::SinglePassConfig, dispatch, single_pass},
    vr::{FreezeMode, ResolutionProgress, pose_control},
};
pub(crate) type Mode = RenderMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticProfile {
    Native,
    ControlledV1,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct VisualMetadata {
    pub generation: u64,
    pub mode: Option<RenderMode>,
    pub profile: DiagnosticProfile,
    pub ready: bool,
    pub paused: bool,
    pub frozen: bool,
    pub error: Option<String>,
    pub warmup_frames: u32,
    pub completed_frames: u32,
    pub engine_dimensions: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ResultState {
    Ready,
    Restored,
    Failed,
}

#[derive(Debug, Serialize)]
struct VisualResult<'a> {
    id: &'a str,
    state: ResultState,
    metadata: &'a VisualMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestId(String);

impl RequestId {
    pub(crate) fn parse(id: &str) -> Result<Self, &'static str> {
        if id.is_empty() || id.len() > 64 {
            return Err("expected 1-64 ASCII letters, digits, underscores, or hyphens");
        }
        if !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err("expected only ASCII letters, digits, underscores, or hyphens");
        }
        Ok(Self(id.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    Configure {
        id: RequestId,
        mode: Mode,
        warmup_frames: u32,
    },
    End {
        id: RequestId,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameObservation {
    pub mode: Mode,
    pub engine_dimensions: (u32, u32),
    pub target_dimensions: (u32, u32),
    pub single_pass: bool,
    pub camera_frozen: bool,
    pub frame_identity: Option<i64>,
    pub generation: u64,
}

pub(crate) fn enqueue(request: Request) {
    let mut state = STATE.lock();
    let id = request.id();
    if state.contains_id(id) || result_path(id).is_some_and(|path| path.exists()) {
        tracing::warn!(id = id.0, "visual control: duplicate request id");
        return;
    }
    if state.pending.len() >= MAX_PENDING {
        let mut result = state.metadata();
        result.ready = false;
        result.error = Some("visual control: the request queue is full".to_owned());
        publish(id, ResultState::Failed, &result);
        return;
    }
    state.pending.push_back(request);
}

pub(crate) fn has_pending() -> bool {
    !STATE.lock().pending.is_empty()
}

pub(crate) fn requires_inline_tail() -> bool {
    let state = STATE.lock();
    state.requires_inline_tail()
}

pub(crate) fn ready() -> bool {
    let state = STATE.lock();
    state.is_ready()
}

pub(crate) fn metadata() -> VisualMetadata {
    STATE.lock().metadata()
}

pub(crate) fn process_pending() {
    let mut state = STATE.lock();
    let Some(request) = state.pending.pop_front() else {
        return;
    };
    let previous_error = state.error.is_some();
    let active = Active {
        id: request.id().clone(),
        started: Instant::now(),
    };
    if let Some((active, metadata)) = state.start(match &request {
        Request::Configure { .. } => Operation::Configuring(active),
        Request::End { .. } => Operation::Restoring(active),
    }) {
        publish(&active.id, ResultState::Failed, &metadata);
    }
    match request {
        Request::Configure {
            mode,
            warmup_frames,
            ..
        } => {
            if state.saved.is_none() {
                state.saved = Some(CONFIG.lock().clone());
                pose_control::clear();
            }
            let first = state.mode.is_none();
            let previous_sps = Config::lock_query(|config| config.stereo.single_pass.enabled);
            apply_profile(&mut CONFIG.lock(), mode);
            CLOCK_HELD.store(true, Ordering::Release);
            unsafe {
                if let Some(clock) = Clock::get() {
                    clock.Pause(true);
                }
            }
            state.mode = Some(mode);
            dispatch::set_mode(mode);
            state.warmup_frames = warmup_frames;
            single_pass::refresh_config_flags();
            if mode == Mode::SinglePass
                && single_pass::probe_if_needed() != single_pass::Capability::Supported
            {
                state.fail(
                    "visual control: the device does not support single-pass viewport routing"
                        .to_owned(),
                );
            } else if (first || previous_error || previous_sps != (mode == Mode::SinglePass))
                && let Err(error) = shader::reload_now()
            {
                state.fail(error.to_string());
            }
        }
        Request::End { .. } => {
            state.mode = None;
            CLOCK_HELD.store(false, Ordering::Release);
            dispatch::set_mode(RenderMode::Native);
            if let Some(saved) = state.saved.as_ref() {
                *CONFIG.lock() = saved.clone();
                single_pass::refresh_config_flags();
                if let Err(error) = shader::reload_now() {
                    state.fail(error.to_string());
                    return;
                }
                pose_control::clear();
            }
            state.warmup_frames = 1;
        }
    }
}

pub(crate) fn frame_complete() {
    let mut state = STATE.lock();
    if state.mode.is_none() {
        if matches!(state.operation, Operation::Restoring(_)) {
            let resolution = crate::vr::resolution_progress();
            let config = CONFIG.lock();
            let paused = unsafe { Clock::get().map(|clock| clock.m_Stop) };
            if let Some(active) = state.complete_restore(
                resolution,
                paused,
                config.director.paused,
                config.vr.freeze_mode != FreezeMode::Off,
            ) {
                publish(&active.id, ResultState::Restored, &state.metadata());
            }
        }
        return;
    }
    let resolution = crate::vr::resolution_status();
    let observed = FrameObservation {
        mode: dispatch::mode(),
        engine_dimensions: resolution.engine.unwrap_or_default(),
        target_dimensions: resolution.target.unwrap_or_default(),
        single_pass: single_pass::active() && single_pass::double_wide_active(),
        camera_frozen: pose_control::current().is_some()
            && Config::lock_query(|config| config.vr.freeze_mode == FreezeMode::FullCamera),
        frame_identity: (crate::hooks::in_gameplay() && crate::vr::is_running_published())
            .then(crate::vr::capture_views)
            .flatten()
            .map(|views| views.predicted_display_time_ns),
        generation: state.generation,
    };
    let paused = unsafe {
        Clock::get().is_some_and(|clock| clock.m_Stop && clock.GetSPF(false) <= Clock::PAUSED_SPF)
    };
    state.complete(observed, paused);
    if state.ready && matches!(state.operation, Operation::Configuring(_)) {
        let Operation::Configuring(active) =
            std::mem::replace(&mut state.operation, Operation::Idle)
        else {
            unreachable!();
        };
        publish(&active.id, ResultState::Ready, &state.metadata());
    }
}

pub(crate) fn tick() {
    let mut state = STATE.lock();
    if let Some(active) = state.expire(Instant::now()) {
        publish(&active.id, ResultState::Failed, &state.metadata());
    }
}

pub(crate) fn clock_held() -> bool {
    CLOCK_HELD.load(Ordering::Acquire)
}

pub(crate) fn shutdown() {
    CLOCK_HELD.store(false, Ordering::Release);
    let mut state = STATE.lock();
    while let Some(request) = state.pending.pop_front() {
        let mut metadata = state.metadata();
        metadata.ready = false;
        metadata.error = Some("visual control: the payload is shutting down".to_owned());
        publish(request.id(), ResultState::Failed, &metadata);
    }
    if let Some(saved) = state.saved.take() {
        *CONFIG.lock() = saved;
        pose_control::clear();
    }
    state.mode = None;
    dispatch::set_mode(RenderMode::Native);
    state.ready = false;
    state.fail("visual control: the payload is shutting down".to_owned());
    state.operation = Operation::Idle;
}

const MAX_PENDING: usize = 16;
const TIMEOUT: Duration = Duration::from_secs(60);

static CLOCK_HELD: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State::new());

struct State {
    pending: VecDeque<Request>,
    saved: Option<Config>,
    operation: Operation,
    mode: Option<Mode>,
    generation: u64,
    ready: bool,
    warmup_frames: u32,
    completed_frames: u32,
    last_dimensions: Option<(u32, u32)>,
    last_frame: Option<i64>,
    paused: bool,
    frozen: bool,
    error: Option<String>,
}

struct Active {
    id: RequestId,
    started: Instant,
}

enum Operation {
    Idle,
    Configuring(Active),
    Restoring(Active),
    RestoreFailed(RequestId),
}

impl Operation {
    fn id(&self) -> Option<&RequestId> {
        match self {
            Self::Idle => None,
            Self::Configuring(active) | Self::Restoring(active) => Some(&active.id),
            Self::RestoreFailed(id) => Some(id),
        }
    }

    fn is_restoring(&self) -> bool {
        matches!(self, Self::Restoring(_) | Self::RestoreFailed(_))
    }
}

impl Request {
    fn id(&self) -> &RequestId {
        match self {
            Self::Configure { id, .. } | Self::End { id } => id,
        }
    }
}

impl State {
    const fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            saved: None,
            operation: Operation::Idle,
            mode: None,
            generation: 0,
            ready: false,
            warmup_frames: 0,
            completed_frames: 0,
            last_dimensions: None,
            last_frame: None,
            paused: false,
            frozen: false,
            error: None,
        }
    }

    fn metadata(&self) -> VisualMetadata {
        VisualMetadata {
            generation: self.generation,
            mode: self.mode,
            profile: if self.mode.is_some() {
                DiagnosticProfile::ControlledV1
            } else {
                DiagnosticProfile::Native
            },
            ready: self.is_ready(),
            paused: self.paused,
            frozen: self.frozen,
            error: self.error.clone(),
            warmup_frames: self.warmup_frames,
            completed_frames: self.completed_frames,
            engine_dimensions: self.last_dimensions,
        }
    }

    fn complete(&mut self, observed: FrameObservation, paused: bool) {
        if observed.generation != self.generation {
            return;
        }
        if observed.frame_identity.is_some() && observed.frame_identity == self.last_frame {
            return;
        }
        self.last_frame = observed.frame_identity;
        self.paused = paused;
        self.frozen = observed.camera_frozen;
        self.last_dimensions = Some(observed.engine_dimensions);
        let matches = observed.frame_identity.is_some()
            && self.mode == Some(observed.mode)
            && observed.single_pass == (observed.mode == Mode::SinglePass)
            && observed.engine_dimensions.0 > 0
            && observed.engine_dimensions.1 > 0
            && observed.engine_dimensions == observed.target_dimensions
            && paused
            && observed.camera_frozen;
        if !matches {
            self.completed_frames = 0;
            self.ready = false;
            return;
        }
        if self.error.is_none() {
            self.completed_frames = self.completed_frames.saturating_add(1);
            self.ready = self.completed_frames >= self.warmup_frames.max(1);
        }
    }

    fn is_ready(&self) -> bool {
        (self.mode.is_none() || self.ready)
            && !self.operation.is_restoring()
            && self.error.is_none()
            && self.pending.is_empty()
    }

    fn requires_inline_tail(&self) -> bool {
        self.mode.is_some() || self.operation.is_restoring() || !self.pending.is_empty()
    }

    fn contains_id(&self, id: &RequestId) -> bool {
        self.pending.iter().any(|pending| pending.id() == id) || self.operation.id() == Some(id)
    }

    fn start(&mut self, operation: Operation) -> Option<(Active, VisualMetadata)> {
        let superseded = self
            .fail_operation("visual control: superseded by another request".to_owned())
            .map(|active| (active, self.metadata()));
        self.operation = operation;
        self.generation += 1;
        self.ready = false;
        self.error = None;
        self.completed_frames = 0;
        self.last_dimensions = None;
        self.last_frame = None;
        superseded
    }

    fn complete_restore(
        &mut self,
        resolution: ResolutionProgress,
        paused: Option<bool>,
        expected_paused: bool,
        frozen: bool,
    ) -> Option<Active> {
        if !matches!(self.operation, Operation::Restoring(_))
            || self.error.is_some()
            || !resolution.settled()
            || paused != Some(expected_paused)
        {
            return None;
        }
        self.saved = None;
        self.paused = expected_paused;
        self.frozen = frozen;
        self.last_dimensions = resolution.engine;
        self.ready = true;
        let Operation::Restoring(active) = std::mem::replace(&mut self.operation, Operation::Idle)
        else {
            unreachable!();
        };
        Some(active)
    }

    fn expire(&mut self, now: Instant) -> Option<Active> {
        let error = match &self.operation {
            Operation::Configuring(active) if now.duration_since(active.started) > TIMEOUT => {
                "visual control: timed out waiting for matching completed world frames"
            }
            Operation::Restoring(active) if now.duration_since(active.started) > TIMEOUT => {
                "visual control: timed out waiting for restoration"
            }
            _ => return None,
        };
        self.fail_operation(error.to_owned())
    }

    fn fail(&mut self, error: String) {
        if let Some(active) = self.fail_operation(error) {
            publish(&active.id, ResultState::Failed, &self.metadata());
        }
    }

    fn fail_operation(&mut self, error: String) -> Option<Active> {
        self.ready = false;
        self.error = Some(error);
        let operation = std::mem::replace(&mut self.operation, Operation::Idle);
        let active = match operation {
            Operation::Configuring(active) => active,
            Operation::Restoring(active) => {
                self.operation = Operation::RestoreFailed(active.id.clone());
                active
            }
            operation => {
                self.operation = operation;
                return None;
            }
        };
        Some(active)
    }
}

fn apply_profile(config: &mut Config, mode: Mode) {
    config.director.paused = true;
    config.director.clean_view = true;
    config.director.clean_view_scope = crate::director::clean_view::CleanViewScope::Everything;
    config.vr.freeze_mode = FreezeMode::FullCamera;
    config.vr.native_resolution = true;
    config.fsr.enabled = false;
    config.foveation.enabled = false;
    config.exposure.force = true;
    config.exposure.forced_value = 0.11;
    config.post_fx.skip_motion_blur = true;
    config.post_fx.skip_motion_blur_recon = true;
    config.post_fx.skip_dof = true;
    config.post_fx.skip_fade = true;
    config.post_fx.skip_glare = true;
    config.post_fx.skip_player_damage = true;
    config.post_fx.skip_sun_halo = true;
    config.post_fx.skip_histogram = true;
    config.stereo.enabled = true;
    config.stereo.cameras = true;
    config.stereo.disable_ssao = true;
    config.stereo.skip_ssr = true;
    config.stereo.skip_gi = true;
    config.stereo.symmetrize_eye_frusta = false;
    config.stereo.mirror_eye0_to_both = false;
    config.stereo.skip_pass_range_enabled = false;
    config.stereo.freeze_shadow_maps = false;
    config.stereo.disable_sun_shadows = false;
    config.stereo.ssao_eye0_only = false;
    config.far_field.enabled = false;
    let on = mode == Mode::SinglePass;
    config.stereo.single_pass = SinglePassConfig::new();
    let sps = &mut config.stereo.single_pass;
    sps.enabled = on;
    sps.dual_eye = on;
    sps.collapse = on;
    sps.double_wide = on;
    sps.reproject = on;
    sps.reproject_camera_only = on;
    sps.terrain = on;
    sps.tree_impostors = on;
    sps.bark = on;
    sps.foliage = on;
    sps.occluder = on;
    sps.patch_dryrun = false;
}

fn result_path(id: &RequestId) -> Option<std::path::PathBuf> {
    crate::config::paths::dir().map(|dir| dir.join("visual").join(format!("{}.json", id.0)))
}

fn publish(id: &RequestId, state: ResultState, metadata: &VisualMetadata) {
    let Some(path) = result_path(id) else {
        tracing::error!("visual control: no configuration directory for the result");
        return;
    };
    let result = VisualResult {
        id: &id.0,
        state,
        metadata,
    };
    if let Err(error) = write_result(&path, &result) {
        tracing::error!(path = %path.display(), "visual control: {error:#}");
    }
}

fn write_result(path: &Path, result: &VisualResult<'_>) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("visual control: the result has no parent directory")?;
    std::fs::create_dir_all(parent).context("visual control: create the result directory")?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(result).context("visual control: encode the result")?;
    std::fs::write(&temporary, bytes).context("visual control: write the temporary result")?;
    std::fs::rename(&temporary, path).context("visual control: publish the result")?;
    Ok(())
}

#[cfg(test)]
mod tests;
