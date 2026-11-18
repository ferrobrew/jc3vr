use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::Ordering,
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

use anyhow::Context as _;
use parking_lot::Mutex;
pub(crate) mod bound_buffer;
pub(crate) mod constants;
pub(crate) mod resolve_probe;
pub(crate) mod stages;
pub(crate) mod types;

use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};

use crate::{
    config::Config,
    image_writer::{self, ImageTarget, WriteOutcome},
    screenshot::{
        CaptureSource,
        harness::types::{
            CameraInput, CaptureKind, CaptureMetadata, CaptureState, CapturedFrame,
            ComparisonInputs, EyeInput, Readiness,
        },
        read_back,
    },
    stereo::dispatch::{self, RenderMode},
    testing::visual::{HISTORY_FRAMES, VisualMetadata},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestId(String);

impl RequestId {
    pub(crate) fn parse(id: &str) -> Result<Self, &'static str> {
        if id.is_empty() || id.len() > 64 {
            return Err("expected 1–64 ASCII letters, digits, underscores, or hyphens");
        }
        if !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err("expected only ASCII letters, digits, underscores, or hyphens");
        }
        Ok(Self(id.to_owned()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WaterReflection {
    ZeroWeight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SunDepthClip {
    DisabledRight,
}

pub(crate) struct CaptureRequest {
    pub water_reflection: Option<WaterReflection>,
    pub water_inputs: bool,
    pub sun_inputs: bool,
    pub resolve_inputs: bool,
    pub sun_depth_clip: Option<SunDepthClip>,
}

pub(crate) fn request(id: RequestId, options: CaptureRequest) {
    if options.water_reflection.is_some() {
        if crate::stereo::dispatch::mode() != RenderMode::Multipass {
            reject(&id, "water reflection treatment: multipass is required");
            return;
        }
        let water_inputs = options.water_inputs;
        let identity = crate::stereo::single_pass::shader_identity::status();
        if !water_inputs || !identity.requested || identity.status != "complete" {
            reject(
                &id,
                "water reflection treatment: water inputs and a complete shader identity census are required",
            );
            return;
        }
    }
    request_with_phase(id, None, options);
}

pub(crate) fn request_motion(id: RequestId, phase: u32, options: CaptureRequest) {
    if options.water_reflection.is_some() {
        reject(
            &id,
            "water reflection treatment: motion capture is not supported",
        );
        return;
    }
    let Some(period) = crate::testing::visual::motion_period() else {
        reject(
            &id,
            "screenshot harness: motion capture requires an active visual motion",
        );
        return;
    };
    if phase >= period {
        reject(
            &id,
            &format!(
                "screenshot harness: motion capture phase {phase} is outside the active motion \
                 period {period}"
            ),
        );
        return;
    }
    request_with_phase(id, Some(phase), options);
}

fn request_with_phase(id: RequestId, target_phase: Option<u32>, options: CaptureRequest) {
    let Some(folder) = crate::config::paths::dir().map(|dir| dir.join("captures")) else {
        tracing::error!("screenshot harness: no configuration directory");
        return;
    };
    let result_path = folder.join(format!("{}.json", id.0));
    if result_path.exists() {
        tracing::warn!(
            "screenshot harness: id {} already has a result; use a unique id",
            id.0
        );
        return;
    }
    let result = readiness();
    let after = frame_identity();
    let mut state = STATE.lock();
    let was_idle = state.active.is_none();
    admit(&mut state, id, folder, after, result, target_phase);
    if was_idle && let Some(active) = state.active.as_mut() {
        active.water_reflection = options.water_reflection;
        active.water_inputs = options.water_inputs;
        active.sun_inputs = options.sun_inputs;
        active.resolve_inputs = options.resolve_inputs;
        active.sun_depth_clip = options.sun_depth_clip;
    }
    sync_pending(&state);
}

fn reject(id: &RequestId, error: &str) {
    let Some(folder) = crate::config::paths::dir().map(|dir| dir.join("captures")) else {
        tracing::error!("screenshot harness: no configuration directory");
        return;
    };
    let result_path = folder.join(format!("{}.json", id.0));
    if result_path.exists() {
        tracing::warn!(
            "screenshot harness: id {} already has a result; use a unique id",
            id.0
        );
        return;
    }
    if STATE
        .lock()
        .active
        .as_ref()
        .is_some_and(|active| &active.id == id)
    {
        tracing::warn!(
            "screenshot harness: id {} is already outstanding; not overwriting it",
            id.0
        );
        return;
    }
    let result = CaptureMetadata {
        id: id.0.clone(),
        state: CaptureState::Failed,
        readiness: readiness(),
        capture: None,
        error: Some(error.to_owned()),
    };
    publish_logged(&result_path, &result);
}

fn sync_pending(state: &State) {
    let pending = match state.active.as_ref() {
        Some(active) if matches!(active.phase, Phase::Pending) => match active.target_phase {
            Some(phase) => phase,
            None => ANY_PHASE,
        },
        _ => NO_PENDING,
    };
    PENDING_STATE.store(pending, Ordering::Release);
}

fn admit(
    state: &mut State,
    id: RequestId,
    folder: PathBuf,
    after: Option<i64>,
    readiness: Readiness,
    target_phase: Option<u32>,
) {
    let result_path = folder.join(format!("{}.json", id.0));
    let mut result = CaptureMetadata {
        id: id.0.clone(),
        state: CaptureState::Failed,
        readiness,
        capture: None,
        error: None,
    };
    if state.active.as_ref().is_some_and(|active| active.id == id) {
        tracing::warn!(
            "screenshot harness: duplicate outstanding id {} is rejected",
            id.0
        );
        return;
    }
    if state.active.is_some() {
        result.error = Some("screenshot harness: another capture is outstanding".to_owned());
        publish_logged(&result_path, &result);
        return;
    }
    if let Err(error) = std::fs::create_dir_all(&folder) {
        result.error = Some(format!(
            "screenshot harness: creating capture directory: {error}"
        ));
        publish_logged(&result_path, &result);
        return;
    }
    constants::reset();
    resolve_probe::reset();
    state.active = Some(Active {
        id,
        folder,
        started: Instant::now(),
        after,
        phase: Phase::Pending,
        result,
        reported: false,
        target_phase,
        water_reflection: None,
        water_inputs: false,
        sun_inputs: false,
        resolve_inputs: false,
        sun_depth_clip: None,
        diagnostic_frame: None,
    });
}

pub(crate) fn wants_eyes() -> bool {
    match PENDING_STATE.load(Ordering::Acquire) {
        NO_PENDING => false,
        ANY_PHASE => true,
        phase => crate::testing::visual::capture_phase_due(phase),
    }
}

pub(crate) fn diagnostic_capture_id() -> Option<String> {
    if !wants_eyes()
        || !crate::testing::visual::ready()
        || !crate::hooks::in_gameplay()
        || !crate::vr::is_running_published()
        || !crate::stereo::active()
    {
        return None;
    }
    let mut state = STATE.lock();
    let active = state.active.as_mut()?;
    let frame = frame_identity()?;
    if !matches!(active.phase, Phase::Pending) || active.after.is_some_and(|after| frame <= after) {
        return None;
    }
    let chosen = active.diagnostic_frame.get_or_insert(frame);
    (*chosen == frame).then(|| active.id.0.clone())
}

pub(crate) fn water_reflection_zero_weight_requested() -> bool {
    if crate::stereo::dispatch::mode() != RenderMode::Multipass || diagnostic_capture_id().is_none()
    {
        return false;
    }
    STATE.lock().active.as_ref().is_some_and(|active| {
        active.water_reflection == Some(WaterReflection::ZeroWeight)
            && matches!(active.phase, Phase::Pending)
    })
}

pub(crate) fn water_inputs_requested() -> bool {
    STATE
        .lock()
        .active
        .as_ref()
        .is_some_and(|active| active.water_inputs && matches!(active.phase, Phase::Pending))
}

pub(crate) fn sun_inputs_requested() -> bool {
    STATE
        .lock()
        .active
        .as_ref()
        .is_some_and(|active| active.sun_inputs && matches!(active.phase, Phase::Pending))
}

pub(crate) fn sun_depth_clip_disabled_right() -> bool {
    if diagnostic_capture_id().is_none() {
        return false;
    }
    STATE.lock().active.as_ref().is_some_and(|active| {
        active.sun_depth_clip == Some(SunDepthClip::DisabledRight)
            && matches!(active.phase, Phase::Pending)
    })
}

pub(crate) fn resolve_inputs_requested() -> bool {
    STATE
        .lock()
        .active
        .as_ref()
        .is_some_and(|active| active.resolve_inputs && matches!(active.phase, Phase::Pending))
}

pub(crate) fn note_eye(eye: usize, copied: bool) {
    if !wants_eyes() {
        return;
    }
    let frame = copied.then(frame_identity).flatten();
    let mut state = STATE.lock();
    if let Some(slot) = state.eyes.get_mut(eye) {
        *slot = frame;
    }
}

pub(crate) fn capture(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: CaptureSource<'_>,
) {
    if !wants_eyes() {
        return;
    }
    let frame = frame_identity();
    let running = crate::vr::is_running_published();
    let gameplay = crate::hooks::in_gameplay();
    let effective_config = crate::config::get();
    let mode = dispatch::mode();
    let selected_eye = mode.selected_eye();
    let target_phase = match PENDING_STATE.load(Ordering::Acquire) {
        NO_PENDING | ANY_PHASE => None,
        phase => Some(phase),
    };
    let visual = match target_phase {
        Some(_) => {
            let Some(frame) = frame else {
                return;
            };
            crate::testing::visual::capture_metadata(frame)
        }
        None => {
            let visual = crate::testing::visual::metadata();
            if mode != RenderMode::Native && !visual.ready {
                return;
            }
            visual
        }
    };
    let mut state = STATE.lock();
    let fresh = state.eyes.map(|eye| frame.is_some() && eye == frame);
    let Some(active) = state.active.as_mut() else {
        return;
    };
    if !matches!(active.phase, Phase::Pending) || active.target_phase != target_phase {
        return;
    }
    if let Some(target) = active.target_phase {
        let Some(frame) = frame else {
            return;
        };
        if let Err(error) = motion_capture_gate(&visual, target, frame) {
            active.result.state = CaptureState::Failed;
            active.result.error = Some(error);
            publish_logged(&active.result_path(), &active.result);
            state.active = None;
            sync_pending(&state);
            return;
        }
    }
    if !eligible(
        running,
        gameplay,
        crate::stereo::active(),
        frame,
        active.after,
        fresh,
        selected_eye,
    ) {
        return;
    }
    PENDING_STATE.store(NO_PENDING, Ordering::Release);
    let source_name = match &source {
        CaptureSource::Full(_) if selected_eye.is_some() => CaptureKind::IsolatedEye,
        CaptureSource::Full(_) => CaptureKind::CollapsedBackBuffer,
        CaptureSource::Eyes(_) => CaptureKind::EyeTextures,
    };
    let capture = match read_back(device, context, source) {
        Ok(capture) => capture,
        Err(error) => {
            active.result.error = Some(format!("screenshot harness: readback: {error:#}"));
            publish_logged(&active.result_path(), &active.result);
            state.active = None;
            return;
        }
    };
    state.ordinal += 1;
    let ordinal = state.ordinal;
    let active = state.active.as_mut().unwrap();
    let png = active.folder.join(format!("{}.capture.png", active.id.0));
    let metadata = png.with_extension("json");
    let eye_width = capture.image.width() / if selected_eye.is_some() { 1 } else { 2 };
    let comparison_inputs = comparison_inputs(
        eye_width,
        capture.image.height(),
        &visual,
        active.target_phase.is_some(),
    );
    active.result.state = CaptureState::Captured;
    active.result.readiness = readiness();
    let (mut stage_records, stage_images) = stages::take(frame.unwrap());
    if active.water_inputs {
        stages::complete_water_inputs(
            &mut stage_records,
            frame.unwrap(),
            &active.id.0,
            required_eyes(selected_eye),
        );
    }
    if active.water_reflection.is_some() && mode == RenderMode::Multipass {
        stages::complete_water_treatment(
            &mut stage_records,
            frame.unwrap(),
            &active.id.0,
            required_eyes(selected_eye),
        );
    }
    if active.sun_inputs {
        stages::complete_sun_inputs(
            &mut stage_records,
            frame.unwrap(),
            &active.id.0,
            required_eyes(selected_eye),
        );
    }
    if active.sun_depth_clip.is_some() && mode == RenderMode::SinglePass {
        stages::complete_sun_treatment(&mut stage_records, frame.unwrap(), &active.id.0);
    }
    let resolve_inputs = active.resolve_inputs.then(|| {
        resolve_probe::take(
            frame.unwrap(),
            visual.generation,
            required_eyes(selected_eye),
        )
    });
    active.result.capture = Some(CapturedFrame {
        png: png.clone(),
        metadata,
        frame_ordinal: ordinal,
        predicted_display_time_ns: frame.unwrap(),
        source: source_name,
        width: capture.image.width(),
        height: capture.image.height(),
        eye_width,
        eye_height: capture.image.height(),
        format: capture.format,
        fresh_eyes: fresh,
        rendered_eyes: required_eyes(selected_eye),
        selected_eye,
        mode,
        visual,
        comparison_inputs,
        stages: stage_records,
        constants: constants::take(frame.unwrap()),
        resolve_inputs,
        water_reflection: active.water_reflection,
        sun_depth_clip: active.sun_depth_clip,
        effective_config,
        single_pass: if mode == RenderMode::SinglePass {
            capture.diagnostics
        } else {
            None
        },
    });
    let sidecar = active.result.clone();
    active.result.state = CaptureState::Failed;
    match image_writer::try_submit(
        ImageTarget::HarnessScreenshot {
            path: png,
            sidecar: Box::new(sidecar),
            stages: stage_images,
        },
        capture.image,
    ) {
        Ok(receiver) => active.phase = Phase::Writing(receiver),
        Err(error) => {
            fail_unwritten_stages(&mut active.result, "image writer: admission failed");
            active.result.error = Some(format!("screenshot harness: writer admission: {error:#}"));
            publish_logged(&active.result_path(), &active.result);
            state.active = None;
        }
    }
}

pub(crate) fn tick() {
    let latest_readiness = wants_eyes().then(readiness);
    let mut state = STATE.lock();
    advance(&mut state, Instant::now(), latest_readiness);
    sync_pending(&state);
}

fn advance(state: &mut State, now: Instant, latest_readiness: Option<Readiness>) {
    let Some(active) = state.active.as_mut() else {
        return;
    };
    if let Phase::Writing(receiver) = &active.phase {
        let outcome = match receiver.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(TryRecvError::Disconnected) => {
                Some(Err("image writer disconnected".to_owned()).into())
            }
            Err(TryRecvError::Empty) => None,
        };
        if let Some(outcome) = outcome {
            if !active.reported {
                if let Some(capture) = &mut active.result.capture
                    && !outcome.stages.is_empty()
                {
                    capture.stages = outcome.stages;
                }
                match outcome.result {
                    Ok(_) => {
                        if let Some(capture) = &mut active.result.capture {
                            capture.resolve_inputs = outcome.resolve_inputs.clone();
                        }
                        let stage_error = active.result.capture.as_ref().is_some_and(|capture| {
                            capture.stages.iter().any(|stage| stage.error.is_some())
                        });
                        if stage_error {
                            active.result.error = Some(
                                "screenshot harness: stage artifact capture or write failed"
                                    .to_owned(),
                            );
                        } else {
                            active.result.state = CaptureState::Completed;
                        }
                    }
                    Err(error) => {
                        fail_unwritten_stages(&mut active.result, &error);
                        active.result.error = Some(error);
                    }
                }
                publish_logged(&active.result_path(), &active.result);
            }
            state.active = None;
            return;
        }
    }
    if !active.reported && now.duration_since(active.started) >= TIMEOUT {
        let pending = matches!(active.phase, Phase::Pending);
        if pending && let Some(latest) = latest_readiness {
            active.result.readiness = latest;
        }
        active.result.state = CaptureState::Failed;
        active.result.error = Some(
            if pending {
                "screenshot harness: timed out waiting for gameplay and a fresh rendered XR pair"
            } else {
                "screenshot harness: timed out waiting for PNG and metadata writes"
            }
            .to_owned(),
        );
        if !pending {
            fail_unwritten_stages(
                &mut active.result,
                "image writer: stage write completion timed out",
            );
        }
        publish_logged(&active.result_path(), &active.result);
        active.reported = true;
        if pending {
            state.active = None;
        }
    }
}

const TIMEOUT: Duration = Duration::from_secs(30);
const NO_PENDING: u32 = u32::MAX;
const ANY_PHASE: u32 = u32::MAX - 1;
static PENDING_STATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(NO_PENDING);
static STATE: Mutex<State> = Mutex::new(State {
    active: None,
    eyes: [None; 2],
    ordinal: 0,
});

struct State {
    active: Option<Active>,
    eyes: [Option<i64>; 2],
    ordinal: u64,
}

struct Active {
    id: RequestId,
    folder: PathBuf,
    started: Instant,
    after: Option<i64>,
    phase: Phase,
    result: CaptureMetadata,
    reported: bool,
    target_phase: Option<u32>,
    water_reflection: Option<WaterReflection>,
    water_inputs: bool,
    sun_inputs: bool,
    resolve_inputs: bool,
    sun_depth_clip: Option<SunDepthClip>,
    diagnostic_frame: Option<i64>,
}

impl Active {
    fn result_path(&self) -> PathBuf {
        self.folder.join(format!("{}.json", self.id.0))
    }
}

enum Phase {
    Pending,
    Writing(Receiver<WriteOutcome>),
}

fn fail_unwritten_stages(result: &mut CaptureMetadata, error: &str) {
    if let Some(capture) = &mut result.capture {
        for stage in &mut capture.stages {
            if stage.png.is_none() && stage.raw.is_none() && stage.error.is_none() {
                stage.error = Some(format!("screenshot stages: {error}"));
            }
        }
    }
}

fn frame_identity() -> Option<i64> {
    crate::vr::capture_views().map(|views| views.predicted_display_time_ns)
}

fn eligible(
    running: bool,
    gameplay: bool,
    stereo: bool,
    frame: Option<i64>,
    after: Option<i64>,
    fresh: [bool; 2],
    selected_eye: Option<usize>,
) -> bool {
    running
        && gameplay
        && stereo
        && frame.is_some()
        && frame != after
        && required_eyes_fresh(selected_eye, fresh)
}

fn required_eyes(selected: Option<usize>) -> [bool; 2] {
    match selected {
        Some(0) => [true, false],
        Some(1) => [false, true],
        _ => [true, true],
    }
}

fn required_eyes_fresh(selected: Option<usize>, fresh: [bool; 2]) -> bool {
    required_eyes(selected)
        .into_iter()
        .zip(fresh)
        .all(|(required, fresh)| !required || fresh)
}

fn motion_capture_gate(visual: &VisualMetadata, target: u32, frame: i64) -> Result<(), String> {
    if let Some(error) = &visual.error {
        return Err(error.clone());
    }
    if !visual.ready {
        return Err("screenshot harness: visual control is not ready".to_owned());
    }
    let Some(motion) = &visual.motion else {
        return Err(
            "screenshot harness: motion capture requires an active visual motion".to_owned(),
        );
    };
    if motion.phase != target {
        return Err(format!(
            "screenshot harness: motion capture requested phase {target}, but the frame rendered \
             phase {}",
            motion.phase
        ));
    }
    if motion.rendered_frame != frame {
        return Err(format!(
            "screenshot harness: motion capture frame {frame} does not match the scheduler's \
             rendered frame {}",
            motion.rendered_frame
        ));
    }
    if motion.cycle < 1 || motion.history.len() < HISTORY_FRAMES {
        return Err(format!(
            "screenshot harness: motion capture phase {target} lacks a completed cycle and \
             {HISTORY_FRAMES} prior motion frames"
        ));
    }
    Ok(())
}

fn comparison_inputs(
    eye_width: u32,
    eye_height: u32,
    visual: &VisualMetadata,
    motion: bool,
) -> ComparisonInputs {
    let camera = if motion {
        visual.pose.as_ref().map(|pose| CameraInput {
            position: pose.applied.position,
            orientation: pose.applied.orientation,
        })
    } else {
        crate::vr::pose_control::current().map(|pose| CameraInput {
            position: pose.position.to_array(),
            orientation: pose.orientation().to_array(),
        })
    };
    let eyes = [0, 1].map(|eye| {
        crate::vr::render_params(eye).map(|params| EyeInput {
            offset: params.local_offset.to_array(),
            orientation_delta: params.orientation_delta.to_array(),
            projection_standard: glam::Mat4::from(params.projection_standard).to_cols_array(),
            projection_reverse_z: glam::Mat4::from(params.projection_reverse_z).to_cols_array(),
        })
    });
    ComparisonInputs {
        build_stamp: crate::BUILD_STAMP,
        profile: visual.profile,
        camera,
        eyes,
        eye_width,
        eye_height,
        world_scale: Config::lock_query(|config| config.vr.world_scale),
    }
}

fn readiness() -> Readiness {
    Readiness {
        gameplay_ready: crate::hooks::in_gameplay(),
        xr_session_running: crate::vr::is_running_published(),
        xr_frame_rendering: frame_identity().is_some(),
        build_stamp: crate::BUILD_STAMP,
    }
}

fn publish_logged(path: &Path, result: &CaptureMetadata) {
    if let Err(error) = publish(path, result) {
        tracing::error!(
            "screenshot harness: publishing {}: {error:#}",
            path.display()
        );
    }
}

fn publish(path: &Path, result: &CaptureMetadata) -> anyhow::Result<()> {
    let temporary = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec_pretty(result).context("screenshot harness: serializing result")?;
    std::fs::write(&temporary, bytes).context("screenshot harness: writing result")?;
    std::fs::rename(&temporary, path).context("screenshot harness: publishing result")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_requests_do_not_coalesce_or_replace_the_capture() {
        let folder = TestFolder::new();
        let mut state = folder.state(Phase::Pending);
        admit(
            &mut state,
            RequestId::parse("second-id").unwrap(),
            folder.0.clone(),
            None,
            test_readiness(),
            None,
        );
        let result: serde_json::Value =
            serde_json::from_slice(&std::fs::read(folder.0.join("second-id.json")).unwrap())
                .unwrap();
        assert_eq!(result["state"], "failed");
        assert!(result["error"].as_str().unwrap().contains("outstanding"));
        assert_eq!(state.active.as_ref().unwrap().id.0, "test-id");
        admit(
            &mut state,
            RequestId::parse("test-id").unwrap(),
            folder.0.clone(),
            None,
            test_readiness(),
            None,
        );
        assert!(!folder.result().exists());
        assert!(matches!(state.active.unwrap().phase, Phase::Pending));
    }

    #[test]
    fn writer_completion_is_the_only_success_acknowledgement() {
        let folder = TestFolder::new();
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut state = folder.state(Phase::Writing(receiver));
        let now = state.active.as_ref().unwrap().started;
        advance(&mut state, now, None);
        assert!(!folder.result().exists());
        sender
            .send(Ok(folder.0.join("capture.png")).into())
            .unwrap();
        advance(&mut state, now, None);
        let result: serde_json::Value =
            serde_json::from_slice(&std::fs::read(folder.result()).unwrap()).unwrap();
        assert_eq!(result["state"], "completed");
        assert!(state.active.is_none());
    }

    #[test]
    fn writer_failures_and_timeouts_never_acknowledge_success() {
        for writer_failure in [true, false] {
            let folder = TestFolder::new();
            let (sender, receiver) = std::sync::mpsc::channel();
            let mut state = folder.state(Phase::Writing(receiver));
            let started = state.active.as_ref().unwrap().started;
            if writer_failure {
                sender
                    .send(Err("metadata write failed".to_owned()).into())
                    .unwrap();
            }
            advance(&mut state, started + TIMEOUT, None);
            let result: serde_json::Value =
                serde_json::from_slice(&std::fs::read(folder.result()).unwrap()).unwrap();
            assert_eq!(result["state"], "failed");
            assert!(result["error"].is_string());
            if !writer_failure {
                assert!(state.active.is_some());
                sender
                    .send(Ok(folder.0.join("capture.png")).into())
                    .unwrap();
                advance(&mut state, started + TIMEOUT, None);
                let unchanged: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(folder.result()).unwrap()).unwrap();
                assert_eq!(result, unchanged);
            }
            assert!(state.active.is_none());
        }
    }

    #[test]
    fn pending_timeout_includes_latest_readiness_and_releases_admission() {
        let folder = TestFolder::new();
        let mut state = folder.state(Phase::Pending);
        let started = state.active.as_ref().unwrap().started;
        advance(
            &mut state,
            started + TIMEOUT,
            Some(Readiness {
                gameplay_ready: false,
                ..test_readiness()
            }),
        );
        let result: serde_json::Value =
            serde_json::from_slice(&std::fs::read(folder.result()).unwrap()).unwrap();
        assert_eq!(result["state"], "failed");
        assert_eq!(result["gameplay_ready"], false);
        assert!(state.active.is_none());
    }

    fn test_readiness() -> Readiness {
        Readiness {
            gameplay_ready: true,
            xr_session_running: true,
            xr_frame_rendering: true,
            build_stamp: "test-build",
        }
    }

    struct TestFolder(PathBuf);

    impl TestFolder {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "jc3vr-harness-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn result(&self) -> PathBuf {
            self.0.join("test-id.json")
        }

        fn state(&self, phase: Phase) -> State {
            State {
                active: Some(Active {
                    id: RequestId::parse("test-id").unwrap(),
                    folder: self.0.clone(),
                    started: Instant::now(),
                    after: None,
                    phase,
                    result: CaptureMetadata {
                        id: "test-id".to_owned(),
                        state: CaptureState::Failed,
                        readiness: test_readiness(),
                        capture: None,
                        error: None,
                    },
                    reported: false,
                    target_phase: None,
                    water_reflection: None,
                    water_inputs: false,
                    sun_inputs: false,
                    resolve_inputs: false,
                    sun_depth_clip: None,
                    diagnostic_frame: None,
                }),
                eyes: [None; 2],
                ordinal: 0,
            }
        }
    }

    impl Drop for TestFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn isolated_capture_requires_only_the_selected_physical_eye() {
        for eye in 0..2 {
            let fresh = required_eyes(Some(eye));
            assert!(eligible(
                true,
                true,
                true,
                Some(2),
                Some(1),
                fresh,
                Some(eye)
            ));
            assert!(!eligible(
                true,
                true,
                true,
                Some(2),
                Some(1),
                fresh,
                Some(1 - eye)
            ));
        }
    }

    #[test]
    fn capture_requires_gameplay_a_live_frame_and_two_fresh_eyes() {
        assert!(eligible(
            true,
            true,
            true,
            Some(2),
            Some(1),
            [true; 2],
            None
        ));
        for fresh in [[false, false], [true, false], [false, true]] {
            assert!(!eligible(true, true, true, Some(2), Some(1), fresh, None));
        }
        assert!(!eligible(
            false,
            true,
            true,
            Some(2),
            Some(1),
            [true; 2],
            None
        ));
        assert!(!eligible(
            true,
            false,
            true,
            Some(2),
            Some(1),
            [true; 2],
            None
        ));
        assert!(!eligible(
            true,
            true,
            false,
            Some(2),
            Some(1),
            [true; 2],
            None
        ));
        assert!(!eligible(true, true, true, None, Some(1), [true; 2], None));
        assert!(!eligible(
            true,
            true,
            true,
            Some(1),
            Some(1),
            [true; 2],
            None
        ));
    }
    #[test]
    fn motion_capture_gate_requires_the_phase_full_cycle_and_eight_prior_frames() {
        assert!(motion_capture_gate(&motion_visual(7, 1, 8, 42), 7, 42).is_ok());
        assert!(motion_capture_gate(&motion_visual(7, 0, 8, 42), 7, 42).is_err());
        assert!(motion_capture_gate(&motion_visual(7, 1, 7, 42), 7, 42).is_err());
        assert!(motion_capture_gate(&motion_visual(6, 1, 8, 42), 7, 42).is_err());
        assert!(motion_capture_gate(&motion_visual(7, 1, 8, 41), 7, 42).is_err());
        let mut errored = motion_visual(7, 1, 8, 42);
        errored.error = Some("visual motion: missing stamp".to_owned());
        assert!(motion_capture_gate(&errored, 7, 42).is_err());
        let mut not_ready = motion_visual(7, 1, 8, 42);
        not_ready.ready = false;
        assert!(motion_capture_gate(&not_ready, 7, 42).is_err());
        let mut no_motion = motion_visual(7, 1, 8, 42);
        no_motion.motion = None;
        assert!(motion_capture_gate(&no_motion, 7, 42).is_err());
    }

    fn motion_visual(phase: u32, cycle: u64, history: usize, frame: i64) -> VisualMetadata {
        use crate::testing::visual::{
            DiagnosticProfile, MotionMetadata, MotionRequest, PoseTransform,
            RequestId as VisualRequestId, motion::MotionHistoryEntry,
        };
        let request = MotionRequest::new(
            VisualRequestId::parse("motion-sweep").unwrap(),
            32,
            10.0,
            5.0,
            2.5,
        )
        .unwrap();
        VisualMetadata {
            generation: 1,
            mode: Some(RenderMode::Multipass),
            profile: DiagnosticProfile::ControlledV2,
            visual_overrides: None,
            shader_identity: crate::stereo::single_pass::shader_identity::status(),
            ready: true,
            paused: true,
            frozen: true,
            error: None,
            warmup_frames: 1,
            completed_frames: 1,
            engine_dimensions: Some((4096, 2048)),
            pose: None,
            environment: None,
            motion: Some(MotionMetadata {
                request,
                phase,
                cycle,
                rendered_frame: frame,
                history: (0..history)
                    .map(|index| MotionHistoryEntry {
                        phase: index as u32,
                        rendered_frame: frame - (history - index) as i64,
                        applied: PoseTransform {
                            position: [0.0; 3],
                            orientation: [0.0, 0.0, 0.0, 1.0],
                        },
                    })
                    .collect(),
            }),
        }
    }
}
