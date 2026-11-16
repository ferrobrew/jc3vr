use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

use anyhow::Context as _;
use parking_lot::Mutex;
pub(crate) mod stages;
pub(crate) mod types;

use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};

use crate::{
    config::Config,
    image_writer::{self, ImageTarget},
    screenshot::{
        CaptureSource,
        harness::types::{
            CameraInput, CaptureKind, CaptureMetadata, CaptureState, CapturedFrame,
            ComparisonInputs, EyeInput, Readiness,
        },
        read_back,
    },
    stereo::dispatch::{self, RenderMode},
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

pub(crate) fn request(id: RequestId) {
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
    admit(&mut state, id, folder, after, result);
    PENDING.store(
        state
            .active
            .as_ref()
            .is_some_and(|a| matches!(a.phase, Phase::Pending)),
        Ordering::Release,
    );
}

fn admit(
    state: &mut State,
    id: RequestId,
    folder: PathBuf,
    after: Option<i64>,
    readiness: Readiness,
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
    state.active = Some(Active {
        id,
        folder,
        started: Instant::now(),
        after,
        phase: Phase::Pending,
        result,
        reported: false,
    });
}

pub(crate) fn wants_eyes() -> bool {
    PENDING.load(Ordering::Acquire)
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
    let visual = crate::testing::visual::metadata();
    if mode != RenderMode::Native && !crate::testing::visual::ready() {
        return;
    }
    let mut state = STATE.lock();
    let fresh = state.eyes.map(|eye| frame.is_some() && eye == frame);
    let Some(active) = state.active.as_mut() else {
        return;
    };
    if !matches!(active.phase, Phase::Pending) {
        return;
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
    PENDING.store(false, Ordering::Release);
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
    let comparison_inputs = comparison_inputs(eye_width, capture.image.height(), &visual);
    active.result.state = CaptureState::Captured;
    active.result.readiness = readiness();
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
        stages: stages::take(frame.unwrap()),
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
        },
        capture.image,
    ) {
        Ok(receiver) => active.phase = Phase::Writing(receiver),
        Err(error) => {
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
    PENDING.store(
        state
            .active
            .as_ref()
            .is_some_and(|a| matches!(a.phase, Phase::Pending)),
        Ordering::Release,
    );
}

fn advance(state: &mut State, now: Instant, latest_readiness: Option<Readiness>) {
    let Some(active) = state.active.as_mut() else {
        return;
    };
    if let Phase::Writing(receiver) = &active.phase {
        let outcome = match receiver.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(TryRecvError::Disconnected) => Some(Err("image writer disconnected".to_owned())),
            Err(TryRecvError::Empty) => None,
        };
        if let Some(outcome) = outcome {
            if !active.reported {
                match outcome {
                    Ok(_) => active.result.state = CaptureState::Completed,
                    Err(error) => active.result.error = Some(error),
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
        publish_logged(&active.result_path(), &active.result);
        active.reported = true;
        if pending {
            state.active = None;
        }
    }
}

const TIMEOUT: Duration = Duration::from_secs(30);
static PENDING: AtomicBool = AtomicBool::new(false);
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
}

impl Active {
    fn result_path(&self) -> PathBuf {
        self.folder.join(format!("{}.json", self.id.0))
    }
}

enum Phase {
    Pending,
    Writing(Receiver<Result<PathBuf, String>>),
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

fn comparison_inputs(
    eye_width: u32,
    eye_height: u32,
    visual: &crate::testing::visual::VisualMetadata,
) -> ComparisonInputs {
    let camera = crate::vr::pose_control::current().map(|pose| CameraInput {
        position: pose.position.to_array(),
        orientation: pose.orientation().to_array(),
    });
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
        sender.send(Ok(folder.0.join("capture.png"))).unwrap();
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
                    .send(Err("metadata write failed".to_owned()))
                    .unwrap();
            }
            advance(&mut state, started + TIMEOUT, None);
            let result: serde_json::Value =
                serde_json::from_slice(&std::fs::read(folder.result()).unwrap()).unwrap();
            assert_eq!(result["state"], "failed");
            assert!(result["error"].is_string());
            if !writer_failure {
                assert!(state.active.is_some());
                sender.send(Ok(folder.0.join("capture.png"))).unwrap();
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
}
