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
    vr::{FreezeMode, pose_control},
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
    if state.pending.iter().any(|pending| pending.id() == id)
        || state.active.as_ref().is_some_and(|active| &active.id == id)
        || result_path(id).is_some_and(|path| path.exists())
    {
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
    state.mode.is_some() || state.restoring.is_some() || !state.pending.is_empty()
}

pub(crate) fn ready() -> bool {
    let state = STATE.lock();
    (state.mode.is_none() || state.ready)
        && state.restoring.is_none()
        && state.error.is_none()
        && state.pending.is_empty()
}

pub(crate) fn metadata() -> VisualMetadata {
    STATE.lock().metadata()
}

pub(crate) fn process_pending() {
    let mut state = STATE.lock();
    let Some(request) = state.pending.pop_front() else {
        return;
    };
    if let Some(active) = state.active.take() {
        let mut result = state.metadata();
        result.ready = false;
        result.error = Some("visual control: superseded by another request".to_owned());
        publish(&active.id, ResultState::Failed, &result);
    }
    let previous_error = state.error.is_some();
    state.generation += 1;
    state.ready = false;
    state.error = None;
    state.completed_frames = 0;
    state.last_dimensions = None;
    state.last_frame = None;
    match request {
        Request::Configure {
            id,
            mode,
            warmup_frames,
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
            state.active = Some(Active {
                id,
                started: Instant::now(),
            });
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
        Request::End { id } => {
            CLOCK_HELD.store(false, Ordering::Release);
            if let Some(saved) = state.saved.as_ref() {
                *CONFIG.lock() = saved.clone();
                state.mode = None;
                dispatch::set_mode(RenderMode::Native);
                single_pass::refresh_config_flags();
                if let Err(error) = shader::reload_now() {
                    state.error = Some(error.to_string());
                    publish(&id, ResultState::Failed, &state.metadata());
                    return;
                }
                state.saved = None;
                pose_control::clear();
            }
            state.mode = None;
            dispatch::set_mode(RenderMode::Native);
            state.warmup_frames = 1;
            state.restoring = Some(Active {
                id,
                started: Instant::now(),
            });
        }
    }
}

pub(crate) fn frame_complete() {
    let mut state = STATE.lock();
    if state.mode.is_none() {
        if state.restoring.is_some() {
            let resolution = crate::vr::resolution_status();
            let config = CONFIG.lock();
            let paused = unsafe { Clock::get().is_some_and(|clock| clock.m_Stop) };
            let matches = resolution.engine.is_some()
                && resolution.engine == resolution.target
                && paused == config.director.paused;
            if matches {
                state.paused = paused;
                state.frozen = config.vr.freeze_mode != FreezeMode::Off;
                state.last_dimensions = resolution.engine;
                state.ready = true;
                let active = state.restoring.take().unwrap();
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
    if state.ready
        && let Some(active) = state.active.take()
    {
        publish(&active.id, ResultState::Ready, &state.metadata());
    }
}

pub(crate) fn tick() {
    let mut state = STATE.lock();
    if state
        .active
        .as_ref()
        .is_some_and(|active| active.started.elapsed() > TIMEOUT)
    {
        state.fail(
            "visual control: timed out waiting for matching completed world frames".to_owned(),
        );
    }
}

pub(crate) fn clock_held() -> bool {
    CLOCK_HELD.load(Ordering::Acquire)
}

pub(crate) fn shutdown() {
    CLOCK_HELD.store(false, Ordering::Release);
    let mut state = STATE.lock();
    state.pending.clear();
    if let Some(saved) = state.saved.take() {
        *CONFIG.lock() = saved;
        pose_control::clear();
    }
    state.mode = None;
    dispatch::set_mode(RenderMode::Native);
    state.ready = false;
    if state.active.is_some() {
        state.fail("visual control: the payload is shutting down".to_owned());
    }
}

const MAX_PENDING: usize = 16;
const TIMEOUT: Duration = Duration::from_secs(60);

static CLOCK_HELD: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State::new());

struct State {
    pending: VecDeque<Request>,
    saved: Option<Config>,
    active: Option<Active>,
    restoring: Option<Active>,
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
            active: None,
            restoring: None,
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
            ready: (self.mode.is_none() || self.ready)
                && self.restoring.is_none()
                && self.error.is_none()
                && self.pending.is_empty(),
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

    fn fail(&mut self, error: String) {
        self.ready = false;
        self.error = Some(error);
        if let Some(active) = self.active.take() {
            publish(&active.id, ResultState::Failed, &self.metadata());
        }
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
