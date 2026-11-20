use std::{
    collections::VecDeque,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use anyhow::Context as _;
use jc3gi::{
    clock::Clock,
    graphics_engine::{graphics_engine::GraphicsEngine, shadow_manager::get_shadow_system_enabled},
};
use parking_lot::Mutex;
use serde::Serialize;

pub(crate) mod environment;
pub(crate) mod headset;
pub(crate) mod motion;
pub(crate) mod overrides;
pub(crate) mod pose;

pub(crate) use environment::{
    EnvironmentControl, EnvironmentError, EnvironmentMetadata, EnvironmentRequest,
    EnvironmentTable, WorldTimeSnapshot,
};
pub(crate) use motion::{
    AdvanceOutcome, HISTORY_FRAMES, MotionError, MotionMetadata, MotionRequest, MotionState,
    MotionTable, anchor_matches,
};
pub(crate) use overrides::{OverrideError, VisualOverrides, VisualOverridesTable};
pub(crate) use pose::{
    OrientationOffset, PoseError, PoseEvidence, PoseFrame, PoseRequest, PoseTable, PoseTransform,
    TranslationOffset, pose_frame, pose_matches,
};

pub(crate) use crate::stereo::dispatch::RenderMode;
use crate::{
    config::{CONFIG, Config},
    hooks::graphics_engine::{shader, tone_mapping},
    stereo::{config::SinglePassConfig, dispatch, single_pass, switches::ShaderBuild},
    vr::{
        FreezeMode, ResolutionProgress,
        pose_control::{self, PoseValues},
    },
};
pub(crate) type Mode = RenderMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticProfile {
    Native,
    ControlledV3,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct VisualMetadata {
    pub generation: u64,
    pub mode: Option<RenderMode>,
    pub profile: DiagnosticProfile,
    pub shader_identity: crate::stereo::single_pass::shader_identity::CensusStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual_overrides: Option<VisualOverrides>,
    /// The state the live shaders were created under, so a capture shows which creation-time
    /// single-pass rewrites its bytecode carried; `None` when no completed reload established one.
    pub shader_build: Option<ShaderBuild>,
    pub ready: bool,
    pub paused: bool,
    pub frozen: bool,
    pub error: Option<String>,
    pub warmup_frames: u32,
    pub completed_frames: u32,
    pub engine_dimensions: Option<(u32, u32)>,
    pub pose: Option<PoseEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion: Option<MotionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentMetadata>,
    /// The exposure the post chain applied, so captures taken with the engine's own metering can be
    /// checked for one shared exposure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f32>,
    /// The headset-like rig substituted for the located views, when a configuration names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headset: Option<headset::HeadsetGeometry>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
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

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Request {
    Configure {
        id: RequestId,
        mode: Mode,
        warmup_frames: u32,
        pose: Option<PoseRequest>,
        motion: Option<MotionRequest>,
        environment: Option<EnvironmentRequest>,
        shader_identity: bool,
        visual_overrides: Option<VisualOverrides>,
    },
    End {
        id: RequestId,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct FrameObservation {
    pub mode: Mode,
    pub engine_dimensions: (u32, u32),
    pub target_dimensions: (u32, u32),
    pub single_pass: bool,
    pub camera_frozen: bool,
    pub horizon_map_ready: bool,
    pub shadow_override_ready: bool,
    /// Whether the live shaders were created under the state the configuration asks for now.
    pub shaders_current: bool,
    pub frame_identity: Option<i64>,
    pub generation: u64,
    pub pose: Option<PoseEvidence>,
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

pub(crate) fn is_active() -> bool {
    STATE.lock().is_active()
}

pub(crate) fn metadata() -> VisualMetadata {
    STATE.lock().metadata()
}

pub(crate) fn motion_period() -> Option<u32> {
    let state = STATE.lock();
    state.mode?;
    state
        .requested_motion
        .as_ref()
        .map(MotionRequest::period_frames)
}

pub(crate) fn capture_phase_due(target: u32) -> bool {
    STATE
        .lock()
        .motion
        .as_ref()
        .is_some_and(|motion| motion.capture_due(target))
}

pub(crate) fn capture_metadata(frame: i64) -> VisualMetadata {
    let state = STATE.lock();
    let Some(motion) = state.motion.as_ref() else {
        return state.metadata();
    };
    let mut metadata = state.metadata();
    metadata.motion = Some(motion.metadata(frame));
    metadata.pose = None;
    if !motion.running() {
        metadata.ready = false;
        return metadata;
    }
    let Some(anchor) = motion.anchor() else {
        metadata.ready = false;
        metadata.error = Some("visual motion: the scheduler has no anchor".to_owned());
        return metadata;
    };
    let Some((base, applied)) = pose_control::rendered_full_camera(frame) else {
        metadata.ready = false;
        metadata.error = Some(format!(
            "visual motion: frame {frame} has no rendered camera stamp"
        ));
        return metadata;
    };
    let phase = motion.phase();
    let expected = motion.request().target(anchor, phase);
    if !anchor_matches(anchor, base) || !pose_matches(applied, expected) {
        metadata.ready = false;
        metadata.error = Some(format!(
            "visual motion: frame {frame} did not render phase {phase}'s pose"
        ));
        return metadata;
    }
    metadata.pose = Some(motion.request().evidence(anchor, phase, applied));
    metadata
}

pub(crate) fn process_pending() {
    let mut state = STATE.lock();
    let Some(request) = state.pending.pop_front() else {
        return;
    };
    let previous_error = state.error.is_some();
    let previous_pose = state.requested_pose.is_some();
    let previous_motion = state.requested_motion.is_some();
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
            pose,
            motion,
            environment,
            shader_identity,
            visual_overrides,
            ..
        } => {
            if let Some(environment) = environment
                && let Err(error) = state.begin_environment(environment)
            {
                state.fail(error.to_string());
                return;
            }
            if state.saved.is_none() {
                state.saved = Some(CONFIG.lock().clone());
                pose_control::clear();
            }
            let reset_pose =
                (previous_pose || previous_motion) && (motion.is_some() || pose.is_none());
            if motion.is_some() {
                state.requested_pose = None;
                state.motion = motion.clone().map(MotionState::new);
                state.requested_motion = motion;
            } else {
                state.requested_pose = pose;
                state.requested_motion = None;
                state.motion = None;
            }
            if reset_pose {
                pose_control::reset();
            }
            let newly_armed = if shader_identity {
                let newly = single_pass::shader_identity::arm();
                single_pass::ensure_viewport_detours();
                if !single_pass::vertex_identity_detour_ready() {
                    single_pass::shader_identity::mark_install_failed();
                    state.fail(
                        "shader identity: vertex shader detour installation failed".to_owned(),
                    );
                    return;
                }
                if !single_pass::ensure_pixel_shader_detour() {
                    single_pass::shader_identity::mark_install_failed();
                    state.fail(
                        "shader identity: pixel shader detour installation failed".to_owned(),
                    );
                    return;
                }
                newly
            } else {
                single_pass::shader_identity::disarm();
                false
            };
            let first = state.mode.is_none();
            let previous_sps = Config::lock_query(|config| config.stereo.single_pass.enabled);
            {
                let mut config = CONFIG.lock();
                apply_profile_with_overrides(&mut config, mode, visual_overrides);
            }
            // Every configuration accumulates FSR's history from its own warm-up, so modes compared
            // against each other start from the same number of frames.
            crate::fsr::request_history_reset();
            state.shadow_sync_required |= sun_shadows_overridden(visual_overrides.as_ref())
                || crate::hooks::game::sun_shadow_override_active();
            state.visual_overrides = visual_overrides;
            headset::set_active(visual_overrides.and_then(|overrides| overrides.headset));
            crate::screenshot::harness::stages::set_geometry_only(
                visual_overrides.is_some_and(|overrides| overrides.stages.is_some()),
            );
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
                return;
            } else if (first
                || previous_error
                || newly_armed
                || previous_sps != (mode == Mode::SinglePass)
                || !shaders_current())
                && let Err(error) = shader::reload_now()
            {
                state.fail(error.to_string());
                return;
            }
            if !shaders_current() {
                state.fail(
                    "visual control: the shaders were not re-created under the requested switches"
                        .to_owned(),
                );
                return;
            }
            if shader_identity && single_pass::shader_identity::status().status != "complete" {
                state.fail("shader identity: shader census is incomplete after reload".to_owned());
            }
        }
        Request::End { .. } => {
            single_pass::shader_identity::disarm();
            if let Err(error) = state.restore_environment_for_end() {
                state.fail_without_environment_retry(error.to_string());
                return;
            }
            state.mode = None;
            state.visual_overrides = None;
            headset::set_active(None);
            crate::screenshot::harness::stages::set_geometry_only(false);
            state.clear_pose();
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
    state.refresh_environment_observation();
    if state.mode.is_none() {
        if matches!(state.operation, Operation::Restoring(_)) {
            let resolution = crate::vr::resolution_progress();
            let config = CONFIG.lock();
            let paused = unsafe { Clock::get().map(|clock| clock.m_Stop) };
            let shadow_ready = !state.shadow_sync_required
                || shadow_override_ready(config.stereo.disable_sun_shadows);
            if let Some(active) = state.complete_restore(
                resolution,
                paused,
                config.director.paused,
                config.vr.freeze_mode != FreezeMode::Off,
                shadow_ready,
            ) {
                let metadata = state.metadata();
                state.environment = None;
                publish(&active.id, ResultState::Restored, &metadata);
            }
        }
        return;
    }
    let resolution = crate::vr::resolution_status();
    let frame_identity = (crate::hooks::in_gameplay() && crate::vr::is_running_published())
        .then(crate::vr::capture_views)
        .flatten()
        .map(|views| views.predicted_display_time_ns);
    let rendered = frame_identity.and_then(pose_control::rendered_full_camera);
    let live = pose_control::current();
    let paused = unsafe {
        Clock::get().is_some_and(|clock| clock.m_Stop && clock.GetSPF(false) <= Clock::PAUSED_SPF)
    };
    if let Some(request) = state.requested_motion.clone() {
        let observed = FrameObservation {
            mode: dispatch::mode(),
            engine_dimensions: resolution.engine.unwrap_or_default(),
            target_dimensions: resolution.target.unwrap_or_default(),
            single_pass: single_pass::active() && single_pass::double_wide_active(),
            camera_frozen: live.is_some()
                && Config::lock_query(|config| config.vr.freeze_mode == FreezeMode::FullCamera),
            horizon_map_ready: horizon_map_ready(),
            shadow_override_ready: !state.shadow_sync_required
                || shadow_override_ready(sun_shadows_overridden(state.visual_overrides.as_ref())),
            shaders_current: shaders_current(),
            frame_identity,
            generation: state.generation,
            pose: None,
        };
        state.complete_motion(&request, observed, rendered, paused);
    } else {
        let requested = state.requested_pose.clone();
        let pose = match pose_frame(requested.as_ref(), frame_identity, rendered, live) {
            PoseFrame::NotRequested => None,
            PoseFrame::Defer => {
                state.defer_for_pose(frame_identity);
                return;
            }
            PoseFrame::Apply(target) => {
                pose_control::set_current(target);
                state.defer_for_pose(frame_identity);
                return;
            }
            PoseFrame::Observed(evidence) => Some(evidence),
        };
        let observed = FrameObservation {
            mode: dispatch::mode(),
            engine_dimensions: resolution.engine.unwrap_or_default(),
            target_dimensions: resolution.target.unwrap_or_default(),
            single_pass: single_pass::active() && single_pass::double_wide_active(),
            camera_frozen: live.is_some()
                && Config::lock_query(|config| config.vr.freeze_mode == FreezeMode::FullCamera),
            horizon_map_ready: horizon_map_ready(),
            shadow_override_ready: !state.shadow_sync_required
                || shadow_override_ready(sun_shadows_overridden(state.visual_overrides.as_ref())),
            shaders_current: shaders_current(),
            frame_identity,
            generation: state.generation,
            pose,
        };
        state.complete(observed, paused);
    }
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
        if let Some(error) = state.cleanup_environment() {
            tracing::warn!("{error}");
        }
        pose_control::reset();
        publish(&active.id, ResultState::Failed, &state.metadata());
    }
}

pub(crate) fn clock_held() -> bool {
    CLOCK_HELD.load(Ordering::Acquire)
}

pub(crate) fn shutdown() {
    single_pass::shader_identity::disarm();
    headset::set_active(None);
    crate::screenshot::harness::stages::set_geometry_only(false);
    CLOCK_HELD.store(false, Ordering::Release);
    let mut state = STATE.lock();
    while let Some(request) = state.pending.pop_front() {
        let mut metadata = state.metadata();
        metadata.ready = false;
        metadata.error = Some("visual control: the payload is shutting down".to_owned());
        publish(request.id(), ResultState::Failed, &metadata);
    }
    if let Some(error) = state.cleanup_environment() {
        tracing::warn!("{error}");
    }
    if let Some(saved) = state.saved.take() {
        *CONFIG.lock() = saved;
        pose_control::clear();
    }
    state.mode = None;
    state.clear_pose();
    dispatch::set_mode(RenderMode::Native);
    state.ready = false;
    state.fail_without_environment_retry("visual control: the payload is shutting down".to_owned());
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
    visual_overrides: Option<VisualOverrides>,
    shadow_sync_required: bool,
    generation: u64,
    ready: bool,
    warmup_frames: u32,
    completed_frames: u32,
    last_dimensions: Option<(u32, u32)>,
    last_frame: Option<i64>,
    paused: bool,
    frozen: bool,
    error: Option<String>,
    requested_pose: Option<PoseRequest>,
    pose: Option<PoseEvidence>,
    requested_motion: Option<MotionRequest>,
    motion: Option<MotionState>,
    environment: Option<EnvironmentControl>,
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
            visual_overrides: None,
            shadow_sync_required: false,
            generation: 0,
            ready: false,
            warmup_frames: 0,
            completed_frames: 0,
            last_dimensions: None,
            last_frame: None,
            paused: false,
            frozen: false,
            error: None,
            requested_pose: None,
            pose: None,
            requested_motion: None,
            motion: None,
            environment: None,
        }
    }

    fn metadata(&self) -> VisualMetadata {
        VisualMetadata {
            generation: self.generation,
            mode: self.mode,
            profile: if self.mode.is_some() {
                DiagnosticProfile::ControlledV3
            } else {
                DiagnosticProfile::Native
            },
            shader_identity: single_pass::shader_identity::status(),
            visual_overrides: self.visual_overrides,
            shader_build: shader::built_with(),
            ready: self.is_ready(),
            paused: self.paused,
            frozen: self.frozen,
            error: self.error.clone(),
            warmup_frames: self.warmup_frames,
            completed_frames: self.completed_frames,
            engine_dimensions: self.last_dimensions,
            pose: self.pose.clone(),
            motion: self
                .motion
                .as_ref()
                .map(|motion| motion.metadata(self.last_frame.unwrap_or(0))),
            environment: self
                .environment
                .as_ref()
                .and_then(EnvironmentControl::observed),
            exposure: tone_mapping::applied_exposure(),
            headset: headset::active(),
        }
    }

    fn begin_environment(&mut self, request: EnvironmentRequest) -> Result<(), EnvironmentError> {
        environment::available()?;
        let saved = match self.environment.as_ref() {
            Some(control) => control.saved(),
            None => WorldTimeSnapshot::capture()?,
        };
        match environment::apply(&request) {
            Ok(observed) => {
                self.record_environment(saved, observed);
                Ok(())
            }
            Err(error) => {
                if self.environment.is_none() {
                    self.environment = Some(EnvironmentControl::pending(saved));
                }
                Err(error)
            }
        }
    }

    fn record_environment(&mut self, saved: WorldTimeSnapshot, observed: EnvironmentMetadata) {
        match self.environment.as_mut() {
            Some(control) => control.update(observed),
            None => self.environment = Some(EnvironmentControl::new(saved, observed)),
        }
    }

    fn refresh_environment_observation(&mut self) {
        let Some(control) = self.environment.as_mut() else {
            return;
        };
        let Some(weather_control) = control.observed().map(|observed| observed.weather_control)
        else {
            return;
        };
        if let Some(observed) = environment::observe(weather_control) {
            control.update(observed);
        }
    }

    fn environment_restore_target(&self) -> Option<WorldTimeSnapshot> {
        self.environment
            .as_ref()
            .filter(|control| control.needs_restore())
            .map(EnvironmentControl::saved)
    }

    fn environment_restored(&mut self, observed: EnvironmentMetadata) {
        if let Some(control) = self.environment.as_mut() {
            control.restored(observed);
        }
    }

    fn cleanup_environment(&mut self) -> Option<EnvironmentError> {
        let Some(saved) = self.environment_restore_target() else {
            self.environment = None;
            return None;
        };
        match environment::restore(saved) {
            Ok(_) => {
                self.environment = None;
                None
            }
            Err(error) => Some(error),
        }
    }

    fn restore_environment_for_end(&mut self) -> Result<(), EnvironmentError> {
        let Some(saved) = self.environment_restore_target() else {
            return Ok(());
        };
        let observed = environment::restore(saved)?;
        self.environment_restored(observed);
        Ok(())
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
        let pose = if let Some(requested) = self.requested_pose.as_ref() {
            observed
                .pose
                .clone()
                .filter(|evidence| requested.matches_evidence(evidence))
        } else {
            None
        };
        let matches = observed.frame_identity.is_some()
            && self.mode == Some(observed.mode)
            && observed.single_pass == (observed.mode == Mode::SinglePass)
            && observed.engine_dimensions.0 > 0
            && observed.engine_dimensions.1 > 0
            && observed.engine_dimensions == observed.target_dimensions
            && paused
            && observed.camera_frozen
            && observed.horizon_map_ready
            && observed.shadow_override_ready
            && observed.shaders_current
            && (self.requested_pose.is_none() || pose.is_some());
        if !matches {
            self.completed_frames = 0;
            self.ready = false;
            self.pose = None;
            return;
        }
        if self.error.is_none() {
            self.completed_frames = self.completed_frames.saturating_add(1);
            self.ready = self.completed_frames >= self.warmup_frames.max(1);
            self.pose = pose;
        } else {
            self.pose = None;
        }
    }

    fn complete_motion(
        &mut self,
        request: &MotionRequest,
        observed: FrameObservation,
        rendered: Option<(PoseValues, PoseValues)>,
        paused: bool,
    ) {
        if observed.generation != self.generation {
            return;
        }
        if observed.frame_identity.is_some() && observed.frame_identity == self.last_frame {
            return;
        }
        let running = self.motion.as_ref().is_some_and(MotionState::running);
        let Some(frame_identity) = observed.frame_identity else {
            if running {
                self.fail("visual motion: the completed frame has no rendered stamp".to_owned());
            } else {
                self.completed_frames = 0;
                self.ready = false;
                self.pose = None;
            }
            return;
        };
        let Some((base, applied)) = rendered else {
            if running {
                self.fail(format!(
                    "visual motion: frame {frame_identity} has no rendered camera stamp"
                ));
            } else {
                self.last_frame = Some(frame_identity);
                self.completed_frames = 0;
                self.ready = false;
                self.pose = None;
            }
            return;
        };
        let matches = self.mode == Some(observed.mode)
            && observed.single_pass == (observed.mode == Mode::SinglePass)
            && observed.engine_dimensions.0 > 0
            && observed.engine_dimensions.1 > 0
            && observed.engine_dimensions == observed.target_dimensions
            && paused
            && observed.camera_frozen
            && observed.horizon_map_ready
            && observed.shadow_override_ready
            && observed.shaders_current;
        if !running {
            if !matches {
                self.last_frame = Some(frame_identity);
                self.completed_frames = 0;
                self.ready = false;
                self.pose = None;
                return;
            }
            if !pose_matches(applied, base) {
                self.fail(format!(
                    "visual motion: warmup frame {frame_identity} did not hold the anchor pose"
                ));
                return;
            }
            self.last_frame = Some(frame_identity);
            self.paused = paused;
            self.frozen = observed.camera_frozen;
            self.last_dimensions = Some(observed.engine_dimensions);
            if self.error.is_some() {
                self.pose = None;
                return;
            }
            self.completed_frames = self.completed_frames.saturating_add(1);
            if self.completed_frames >= self.warmup_frames.max(1) {
                let Some(motion) = self.motion.as_mut() else {
                    return;
                };
                motion.begin(base);
                pose_control::set_current(request.target(base, 0));
                self.pose = Some(request.evidence(base, 0, applied));
                self.ready = true;
            }
            return;
        }
        if !matches {
            self.fail(format!(
                "visual motion: frame {frame_identity} did not complete a matching frame"
            ));
            return;
        }
        let anchor = self
            .motion
            .as_ref()
            .and_then(MotionState::anchor)
            .unwrap_or(base);
        if !anchor_matches(anchor, base) {
            self.fail(format!(
                "visual motion: frame {frame_identity} rendered a different anchor"
            ));
            return;
        }
        let phase = self.motion.as_ref().map(MotionState::phase).unwrap_or(0);
        let outcome = match self
            .motion
            .as_mut()
            .expect("running motion has state")
            .advance(frame_identity, applied)
        {
            Ok(outcome) => outcome,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        let AdvanceOutcome::Advanced(next) = outcome else {
            return;
        };
        pose_control::set_current(next);
        self.pose = Some(request.evidence(anchor, phase, applied));
        self.last_frame = Some(frame_identity);
        self.paused = paused;
        self.frozen = observed.camera_frozen;
        self.last_dimensions = Some(observed.engine_dimensions);
        self.completed_frames = self.completed_frames.saturating_add(1);
        self.ready = true;
    }

    fn defer_for_pose(&mut self, frame_identity: Option<i64>) {
        self.last_frame = frame_identity;
        self.completed_frames = 0;
        self.ready = false;
        self.pose = None;
    }

    fn clear_pose(&mut self) {
        self.requested_pose = None;
        self.pose = None;
        self.requested_motion = None;
        self.motion = None;
    }

    fn is_ready(&self) -> bool {
        (self.mode.is_none() || self.ready)
            && !self.operation.is_restoring()
            && self.error.is_none()
            && self.pending.is_empty()
    }

    fn is_active(&self) -> bool {
        self.mode.is_some()
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
        self.pose = None;
        self.requested_motion = None;
        self.motion = None;
        superseded
    }

    fn complete_restore(
        &mut self,
        resolution: ResolutionProgress,
        paused: Option<bool>,
        expected_paused: bool,
        frozen: bool,
        shadow_ready: bool,
    ) -> Option<Active> {
        if !matches!(self.operation, Operation::Restoring(_))
            || self.error.is_some()
            || !resolution.settled()
            || paused != Some(expected_paused)
            || !shadow_ready
        {
            return None;
        }
        self.saved = None;
        self.shadow_sync_required = false;
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
        let error = match self.cleanup_environment() {
            Some(restore) => format!("{error}; {restore}"),
            None => error,
        };
        self.fail_without_environment_retry(error);
    }

    fn fail_without_environment_retry(&mut self, error: String) {
        if let Some(active) = self.fail_operation(error) {
            publish(&active.id, ResultState::Failed, &self.metadata());
        }
    }

    fn fail_operation(&mut self, error: String) -> Option<Active> {
        self.ready = false;
        self.error = Some(error);
        self.requested_pose = None;
        self.pose = None;
        self.requested_motion = None;
        self.motion = None;
        pose_control::reset();
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

fn shadow_state_matches(disable: bool, setting: Option<bool>, live: bool, active: bool) -> bool {
    let Some(setting) = setting else {
        return false;
    };
    if disable {
        active && !setting && !live
    } else {
        !active && setting == live
    }
}

fn shadow_override_ready(disable: bool) -> bool {
    let setting = unsafe {
        GraphicsEngine::get()
            .and_then(|engine| engine.m_ShadowManager.as_ref())
            .map(|shadow| shadow.m_Enabled)
    };
    let live = unsafe { *get_shadow_system_enabled() };
    shadow_state_matches(
        disable,
        setting,
        live,
        crate::hooks::game::sun_shadow_override_active(),
    )
}

/// Whether both horizon-map slices are loaded for both frame parities.
///
/// The engine keeps two asynchronously loaded slices per parity and stages them as the two
/// `m_HorizonMaps` entries. While one slice is still loading it substitutes the other for both
/// entries (or the fallback texture when neither is loaded), so a capture taken then shades the sky
/// differently from one taken after the load completes. Two distinct, non-fallback maps are the
/// settled state; the blend time is deliberately not consulted because it is legitimately zero at
/// an exact slice boundary.
fn horizon_maps_ready<T: Copy + PartialEq>(
    enabled: bool,
    fallback: Option<T>,
    maps: [[Option<T>; 2]; 2],
) -> bool {
    if !enabled {
        return true;
    }
    let Some(fallback) = fallback else {
        return false;
    };
    (0..2).all(|parity| {
        let (Some(map0), Some(map1)) = (maps[0][parity], maps[1][parity]) else {
            return false;
        };
        map0 != fallback && map1 != fallback && map0 != map1
    })
}

/// The live [`horizon_maps_ready`] state, read from the engine's shadow manager.
fn horizon_map_ready() -> bool {
    let Some(engine) = (unsafe { GraphicsEngine::get() }) else {
        return false;
    };
    let Some(shadow) = (unsafe { engine.m_ShadowManager.as_ref() }) else {
        return false;
    };
    let present = |pointer: *mut jc3gi::graphics_engine::texture::Texture| {
        (!pointer.is_null()).then_some(pointer)
    };
    horizon_maps_ready(
        shadow.m_EnableHorizonMapping,
        present(shadow.m_HorizonMapFallback),
        [
            [
                present(shadow.m_HorizonMaps[0][0]),
                present(shadow.m_HorizonMaps[0][1]),
            ],
            [
                present(shadow.m_HorizonMaps[1][0]),
                present(shadow.m_HorizonMaps[1][1]),
            ],
        ],
    )
}

/// Whether the live shaders were created under the state the configuration asks for now: a
/// configuration that changes a creation-time rewrite (a single-pass switch that rebuilds shaders,
/// or the mode itself) is not captured until a reload has re-created the shader set under it.
fn shaders_current() -> bool {
    shader::built_with() == Some(ShaderBuild::current())
}

fn sun_shadows_overridden(overrides: Option<&VisualOverrides>) -> bool {
    overrides.is_some_and(|overrides| overrides.sun_shadows.is_some())
}

fn apply_profile_with_overrides(
    config: &mut Config,
    mode: Mode,
    overrides: Option<VisualOverrides>,
) {
    apply_profile(config, mode);
    if let Some(overrides) = overrides {
        overrides.apply(config);
    }
}

/// The controlled capture profile: everything that varies with time, input, or the headset is
/// pinned or off, and every scene effect the game ships with stays on, so a capture exercises the
/// shaders a player sees. Bloom, the sun halo, ambient occlusion, screen-space reflections, and
/// global illumination all run; a profile that skipped the bloom also left the composite reading
/// bloom textures nothing refreshed, whose contents depended on the previous capture set.
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
    config.post_fx.skip_dof = false;
    config.post_fx.dof_no_reproject = true;
    config.post_fx.skip_fade = true;
    config.post_fx.skip_glare = false;
    config.post_fx.skip_player_damage = true;
    config.post_fx.skip_sun_halo = false;
    config.post_fx.skip_histogram = true;
    config.stereo.enabled = true;
    config.stereo.cameras = true;
    config.stereo.disable_ssao = false;
    config.stereo.skip_ssr = false;
    config.stereo.skip_gi = false;
    config.stereo.skip_ao_volumes = false;
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
