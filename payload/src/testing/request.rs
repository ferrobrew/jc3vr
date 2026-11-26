//! The request file: a TOML file a script drops into the configuration directory to start or stop
//! the test tools without the debug UI or a key. The game thread looks for it once a second (every
//! frame in a capture harness session), and claims, reads, deletes, and acts on it.
//!
//! ```toml
//! command = "replay"    # record, stop_recording, replay, or stop_replay
//! path = "C:/climb.jc3trk"  # replay: absolute
//! start = "marker"      # replay: marker, bookmark, or in_place
//! pacing = "real_time"  # replay: real_time or frame_locked
//! ```

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::Deserialize;

use crate::{
    config::Config,
    screenshot::harness::snapshot::{SnapshotError, SnapshotRequest, SnapshotTable},
    testing::{
        benchmark::{BenchmarkError, BenchmarkRequest, BenchmarkTable},
        replay::{self, Pacing, ReplayOptions, StartMode},
        stage::{self, StageRequest, StageTable},
        text::{self, TextError},
        visual,
    },
};

/// The request file's name in the configuration directory.
pub(crate) const FILE_NAME: &str = "test_request.toml";

/// Look for the request file and act on it, at most once every [`POLL_INTERVAL`], or every frame
/// in a capture harness session. Game thread.
pub(crate) fn poll() {
    {
        let mut last = LAST_POLL.lock();
        let interval = if harness_session() {
            Duration::ZERO
        } else {
            POLL_INTERVAL
        };
        if last.is_some_and(|at| at.elapsed() < interval) {
            return;
        }
        *last = Some(Instant::now());
    }
    let (enabled, watched) = Config::lock_query(|c| (c.testing.enabled, c.testing.request_file));
    if !enabled || !watched {
        return;
    }
    let Some(path) = crate::config::paths::file(FILE_NAME) else {
        return;
    };
    match take(&path) {
        Ok(Some(text)) => match parse(&path, &text) {
            Ok(request) => act(request),
            Err(error) => tracing::warn!("{error}"),
        },
        Ok(None) => {}
        Err(error) => tracing::warn!("{error}"),
    }
}

/// How often the request file is looked for outside a capture harness session.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Whether the game runs in a capture harness session (`tools/rendering`), whose requests come
/// one after another and should not each wait out [`POLL_INTERVAL`]. A file check is cheap next to
/// a frame.
fn harness_session() -> bool {
    static SESSION: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SESSION.get_or_init(|| std::env::var_os("JC3VR_HARNESS_TOKEN").is_some())
}

static LAST_POLL: Mutex<Option<Instant>> = Mutex::new(None);

/// A request, read.
#[derive(Debug, Clone, PartialEq)]
enum Request {
    Record,
    StopRecording,
    Replay {
        path: PathBuf,
        options: ReplayOptions,
    },
    StopReplay,
    Screenshot {
        id: crate::screenshot::harness::RequestId,
        motion: Option<u32>,
        water_reflection: Option<crate::screenshot::harness::WaterReflection>,
        water_inputs: bool,
        water_tess_draw: Option<u8>,
        patch_draw_ordinal: Option<u8>,
        second_patch_ordinal: Option<u8>,
        water_surface_inputs: bool,
        late_sky_stages: bool,
        draw_census: bool,
        fog_inputs: bool,
        group_sources: bool,
        group_inputs: bool,
        terrain_interval: bool,
        base_sources: bool,
        base_projection: bool,
        group_cards: bool,
        group_residual: bool,
        aa_weights: bool,
        aa_draws: bool,
        lake_owner_four: bool,
        lake_producer: bool,
        lake_gradients: bool,
        lake_waves: bool,
        lake_normals: bool,
        lake_displacement: bool,
        lake_samplers: bool,
        tess_writer: bool,
        tess_owners: bool,
        resolve_shadow: bool,
        dof_inputs: bool,
        fog_generation: bool,
        sun_inputs: bool,
        resolve_inputs: bool,
        resolve_samples: bool,
        ssr_bracket: bool,
        ssr_operands: bool,
        ssr_prepare: bool,
        ssr_history: bool,
        dof_edge_local: Option<bool>,
        card_atlas: Option<bool>,
        group_fade: Option<bool>,
        group_atlas_scale: Option<bool>,
        aa_color_edge: Option<bool>,
        patch63_hull: Option<bool>,
        owner_four_domain: Option<bool>,
        lake_color_mirror: Option<bool>,
        lake_depth_edge: Option<bool>,
        lake_ds_mvp: Option<bool>,
        lake_inner_edge: Option<bool>,
        writer_domain_camera: Option<bool>,
        base_eye_vp: Option<bool>,
        fog_local_uv: Option<bool>,
        terrain_camera: Option<bool>,
        extra_bark_dither: Option<bool>,
        left_atlas_scale: Option<bool>,
        impostor_scale: Option<bool>,
        color_dither: Option<bool>,
        bark_dither: Option<bool>,
        terrain_sources: bool,
        residual_bark: bool,
        left_interval: bool,
        impostor_inputs: bool,
        interval_trace: bool,
        post_geometry: bool,
        bark_color_trace: bool,
        velocity_trace: bool,
        early_depth: bool,
        resolve_eye_camera: Option<bool>,
        proxy_eye: Option<bool>,
        march_eye: Option<bool>,
        veto_eye: Option<bool>,
        grid_eye: Option<bool>,
        history_center: Option<bool>,
        cascade_scale: Option<crate::screenshot::harness::CascadeScale>,
        sun_depth_clip: Option<crate::screenshot::harness::SunDepthClip>,
    },
    Visual(visual::Request),
    Stage(StageRequest),
    Snapshot(SnapshotRequest),
    SnapshotRelease(crate::screenshot::harness::RequestId),
    Benchmark(BenchmarkRequest),
}

/// Why a request file could not be used.
#[derive(Debug)]
struct RequestError {
    path: PathBuf,
    kind: RequestErrorKind,
}

#[derive(Debug)]
enum RequestErrorKind {
    Read(TextError),
    Claim(std::io::Error),
    Remove(std::io::Error),
    Parse(toml::de::Error),
    /// The command needs a path and has none.
    NoPath(Command),
    /// The path is not absolute.
    NotAbsolute,
    InvalidId(&'static str),
    InvalidPose(visual::PoseError),
    InvalidMotion(visual::MotionError),
    InvalidEnvironment(visual::EnvironmentError),
    InvalidStage(stage::StageError),
    InvalidSnapshot(SnapshotError),
    SnapshotOnCommand(Command),
    NoSnapshot,
    InvalidBenchmark(BenchmarkError),
    BenchmarkOnCommand(Command),
    NoBenchmark,
    StageOnCommand(Command),
    NoStage,
    PoseOnCommand(Command),
    AnchorOnCommand(Command),
    MotionOnCommand(Command),
    EnvironmentOnCommand(Command),
    VisualOverridesOnCommand(Command),
    InvalidVisualOverrides(visual::OverrideError),
    ShaderIdentityOnCommand(Command),
    MotionCaptureOnCommand(Command),
    WaterReflectionOnCommand(Command),
    WaterReflectionNeedsInputs,
    WaterInputsOnCommand(Command),
    InvalidPatchOrdinal,
    InvalidWaterDraw,
    InvalidSurfaceInputs,
    InvalidLateSkyStages,
    InvalidDrawCensus,
    InvalidResolveSamples,
    InvalidCascadeScale,
    SunInputsOnCommand(Command),
    ResolveInputsOnCommand(Command),
    SunDepthClipOnCommand(Command),
    SunDepthClipNeedsSunInputs,
    MotionWithPose,
    NoMode,
    InvalidMode,
    InvalidWarmup,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "testing: request file {}: ", self.path.display())?;
        match &self.kind {
            RequestErrorKind::Read(e) => write!(f, "could not read it: {e}"),
            RequestErrorKind::Claim(e) => write!(f, "could not claim it: {e}"),
            RequestErrorKind::Remove(e) => {
                write!(f, "could not remove it, so it is not acted on: {e}")
            }
            RequestErrorKind::Parse(e) => write!(f, "could not parse it: {e}"),
            RequestErrorKind::NoPath(command) => write!(f, "{command:?} needs a path"),
            RequestErrorKind::InvalidId(reason) => write!(f, "invalid request id: {reason}"),
            RequestErrorKind::InvalidPose(error) => write!(f, "{error}"),
            RequestErrorKind::InvalidMotion(error) => write!(f, "{error}"),
            RequestErrorKind::InvalidEnvironment(error) => write!(f, "{error}"),
            RequestErrorKind::InvalidStage(error) => write!(f, "{error}"),
            RequestErrorKind::InvalidSnapshot(error) => write!(f, "{error}"),
            RequestErrorKind::SnapshotOnCommand(command) => {
                write!(f, "only snapshot accepts a snapshot table, not {command:?}")
            }
            RequestErrorKind::NoSnapshot => write!(f, "snapshot needs a snapshot table"),
            RequestErrorKind::InvalidBenchmark(error) => write!(f, "{error}"),
            RequestErrorKind::BenchmarkOnCommand(command) => {
                write!(
                    f,
                    "only benchmark accepts a benchmark table, not {command:?}"
                )
            }
            RequestErrorKind::NoBenchmark => write!(f, "benchmark needs a benchmark table"),
            RequestErrorKind::StageOnCommand(command) => {
                write!(
                    f,
                    "only visual_stage accepts a stage table, not {command:?}"
                )
            }
            RequestErrorKind::NoStage => write!(f, "visual_stage needs a stage table"),
            RequestErrorKind::PoseOnCommand(command) => {
                write!(f, "only visual_configure accepts a pose, not {command:?}")
            }
            RequestErrorKind::AnchorOnCommand(command) => {
                write!(
                    f,
                    "only visual_configure accepts an anchor, not {command:?}"
                )
            }
            RequestErrorKind::MotionOnCommand(command) => {
                write!(
                    f,
                    "only visual_configure accepts a motion table, not {command:?}"
                )
            }
            RequestErrorKind::EnvironmentOnCommand(command) => {
                write!(
                    f,
                    "only visual_configure accepts an environment table, not {command:?}"
                )
            }
            RequestErrorKind::ShaderIdentityOnCommand(command) => write!(
                f,
                "only visual_configure accepts shader_identity, not {command:?}"
            ),
            RequestErrorKind::VisualOverridesOnCommand(command) => {
                write!(
                    f,
                    "only visual_configure accepts visual_overrides, not {command:?}"
                )
            }
            RequestErrorKind::InvalidVisualOverrides(error) => write!(f, "{error}"),
            RequestErrorKind::MotionCaptureOnCommand(command) => {
                write!(
                    f,
                    "only screenshot accepts a motion_capture table, not {command:?}"
                )
            }
            RequestErrorKind::MotionWithPose => {
                write!(
                    f,
                    "visual_configure accepts either a pose or a motion table, not both"
                )
            }
            RequestErrorKind::WaterReflectionOnCommand(command) => write!(
                f,
                "only screenshot accepts water_reflection, not {command:?}"
            ),
            RequestErrorKind::WaterReflectionNeedsInputs => {
                write!(f, "water_reflection requires water_inputs")
            }
            RequestErrorKind::InvalidResolveSamples => write!(
                f,
                "resolve samples require an untimed resolve screenshot without a cascade override"
            ),
            RequestErrorKind::InvalidCascadeScale => write!(
                f,
                "cascade_scale requires a resolve screenshot without motion"
            ),
            RequestErrorKind::InvalidLateSkyStages => {
                write!(f, "late_sky_stages requires a screenshot without motion")
            }
            RequestErrorKind::InvalidDrawCensus => {
                write!(f, "draw_census requires a screenshot without motion")
            }
            RequestErrorKind::InvalidSurfaceInputs => write!(
                f,
                "surface inputs require a selected water draw without motion or incompatible treatments"
            ),
            RequestErrorKind::InvalidPatchOrdinal => write!(
                f,
                "terrain diagnostic: patch_draw_ordinal requires a camera experiment and a positive ordinal"
            ),
            RequestErrorKind::InvalidWaterDraw => write!(
                f,
                "water_tess_draw requires water_inputs and an index below 32"
            ),
            RequestErrorKind::WaterInputsOnCommand(command) => {
                write!(f, "only screenshot accepts water_inputs, not {command:?}")
            }
            RequestErrorKind::SunDepthClipOnCommand(command) => {
                write!(f, "only screenshot accepts sun_depth_clip, not {command:?}")
            }
            RequestErrorKind::SunDepthClipNeedsSunInputs => {
                write!(f, "sun_depth_clip requires sun_inputs")
            }
            RequestErrorKind::ResolveInputsOnCommand(command) => {
                write!(f, "only screenshot accepts resolve_inputs, not {command:?}")
            }
            RequestErrorKind::SunInputsOnCommand(command) => {
                write!(f, "only screenshot accepts sun_inputs, not {command:?}")
            }
            RequestErrorKind::NoMode => write!(f, "visual_configure needs a mode"),
            RequestErrorKind::InvalidMode => {
                write!(f, "visual_configure does not accept the native mode")
            }
            RequestErrorKind::InvalidWarmup => {
                write!(f, "visual_configure warmup_frames exceeds 300")
            }
            RequestErrorKind::NotAbsolute => write!(f, "the path must be absolute"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Command {
    VisualConfigure,
    VisualEnd,
    VisualStage,
    Screenshot,
    Snapshot,
    SnapshotRelease,
    Benchmark,
    Record,
    StopRecording,
    Replay,
    StopReplay,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestFile {
    command: Command,
    id: Option<String>,
    path: Option<String>,
    #[serde(default)]
    start: StartMode,
    #[serde(default)]
    pacing: Pacing,
    mode: Option<visual::RenderMode>,
    warmup_frames: Option<u32>,
    pose: Option<visual::PoseTable>,
    anchor: Option<visual::AnchorTable>,
    motion: Option<visual::MotionTable>,
    environment: Option<visual::EnvironmentTable>,
    stage: Option<StageTable>,
    shader_identity: Option<bool>,
    visual_overrides: Option<visual::VisualOverridesTable>,
    motion_capture: Option<MotionCaptureTable>,
    snapshot: Option<SnapshotTable>,
    benchmark: Option<BenchmarkTable>,
    water_reflection: Option<crate::screenshot::harness::WaterReflection>,
    #[serde(default)]
    water_inputs: bool,
    water_tess_draw: Option<u8>,
    patch_draw_ordinal: Option<u8>,
    second_patch_ordinal: Option<u8>,
    #[serde(default)]
    water_surface_inputs: bool,
    #[serde(default)]
    late_sky_stages: bool,
    #[serde(default)]
    draw_census: bool,
    #[serde(default)]
    fog_inputs: bool,
    #[serde(default)]
    group_sources: bool,
    #[serde(default)]
    group_inputs: bool,
    #[serde(default)]
    terrain_interval: bool,
    #[serde(default)]
    base_sources: bool,
    #[serde(default)]
    base_projection: bool,
    #[serde(default)]
    group_cards: bool,
    #[serde(default)]
    group_residual: bool,
    #[serde(default)]
    aa_weights: bool,
    #[serde(default)]
    aa_draws: bool,
    #[serde(default)]
    lake_owner_four: bool,
    #[serde(default)]
    lake_producer: bool,
    #[serde(default)]
    lake_gradients: bool,
    #[serde(default)]
    lake_waves: bool,
    #[serde(default)]
    lake_normals: bool,
    #[serde(default)]
    lake_displacement: bool,
    #[serde(default)]
    lake_samplers: bool,
    #[serde(default)]
    tess_writer: bool,
    #[serde(default)]
    tess_owners: bool,
    #[serde(default)]
    resolve_shadow: bool,
    #[serde(default)]
    dof_inputs: bool,
    #[serde(default)]
    fog_generation: bool,
    #[serde(default)]
    sun_inputs: bool,
    #[serde(default)]
    resolve_inputs: bool,
    #[serde(default)]
    ssr_bracket: bool,
    #[serde(default)]
    ssr_operands: bool,
    #[serde(default)]
    ssr_prepare: bool,
    #[serde(default)]
    ssr_history: bool,
    dof_edge_local: Option<bool>,
    card_atlas: Option<bool>,
    group_fade: Option<bool>,
    group_atlas_scale: Option<bool>,
    aa_color_edge: Option<bool>,
    patch63_hull: Option<bool>,
    owner_four_domain: Option<bool>,
    lake_color_mirror: Option<bool>,
    lake_depth_edge: Option<bool>,
    lake_ds_mvp: Option<bool>,
    lake_inner_edge: Option<bool>,
    writer_domain_camera: Option<bool>,
    base_eye_vp: Option<bool>,
    fog_local_uv: Option<bool>,
    terrain_camera: Option<bool>,
    extra_bark_dither: Option<bool>,
    left_atlas_scale: Option<bool>,
    impostor_scale: Option<bool>,
    color_dither: Option<bool>,
    bark_dither: Option<bool>,
    #[serde(default)]
    terrain_sources: bool,
    #[serde(default)]
    residual_bark: bool,
    #[serde(default)]
    left_interval: bool,
    #[serde(default)]
    impostor_inputs: bool,
    #[serde(default)]
    interval_trace: bool,
    #[serde(default)]
    post_geometry: bool,
    #[serde(default)]
    bark_color_trace: bool,
    #[serde(default)]
    velocity_trace: bool,
    #[serde(default)]
    early_depth: bool,
    #[serde(default)]
    resolve_samples: bool,
    resolve_eye_camera: Option<bool>,
    proxy_eye: Option<bool>,
    march_eye: Option<bool>,
    veto_eye: Option<bool>,
    grid_eye: Option<bool>,
    history_center: Option<bool>,
    cascade_scale: Option<crate::screenshot::harness::CascadeScale>,
    sun_depth_clip: Option<crate::screenshot::harness::SunDepthClip>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionCaptureTable {
    phase: u32,
}

/// Claim before reading so cancellation and replacement cannot redirect the final unlink.
/// The game thread is the sole consumer; an interrupted claim is discarded, never replayed.
fn take(path: &Path) -> Result<Option<String>, RequestError> {
    claim(path)?.map(ClaimedRequest::read).transpose()
}

struct ClaimedRequest {
    path: PathBuf,
}

impl ClaimedRequest {
    fn read(self) -> Result<String, RequestError> {
        let error = |kind| RequestError {
            path: self.path.clone(),
            kind,
        };
        let text = text::read(&self.path, text::REQUEST_BYTES)
            .map_err(|e| error(RequestErrorKind::Read(e)));
        let removed =
            std::fs::remove_file(&self.path).map_err(|e| error(RequestErrorKind::Remove(e)));
        removed?;
        text
    }
}

fn claim(path: &Path) -> Result<Option<ClaimedRequest>, RequestError> {
    let claimed = path.with_extension("toml.claimed");
    // One fixed claim bounds stale files across restarts. Only the consumer touches this path.
    match std::fs::remove_file(&claimed) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(RequestError {
                path: claimed,
                kind: RequestErrorKind::Remove(error),
            });
        }
    }
    match std::fs::rename(path, &claimed) {
        Ok(()) => Ok(Some(ClaimedRequest { path: claimed })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(RequestError {
            path: path.to_owned(),
            kind: RequestErrorKind::Claim(error),
        }),
    }
}

fn parse(path: &Path, text: &str) -> Result<Request, RequestError> {
    let error = |kind| RequestError {
        path: path.to_owned(),
        kind,
    };
    text::limit("request file", text.len(), text::REQUEST_BYTES)
        .map_err(|e| error(RequestErrorKind::Read(e)))?;
    let file: RequestFile = toml::from_str(text).map_err(|e| error(RequestErrorKind::Parse(e)))?;
    if let Some(path) = &file.path {
        text::string("path", path).map_err(|e| error(RequestErrorKind::Read(e)))?;
    }
    let resolve = |command| -> Result<PathBuf, RequestError> {
        let given = file
            .path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .ok_or_else(|| error(RequestErrorKind::NoPath(command)))?;
        let given = PathBuf::from(given);
        if given.is_absolute() {
            return Ok(given);
        }
        Err(error(RequestErrorKind::NotAbsolute))
    };
    let pose = match file.pose {
        Some(table) => {
            if file.command != Command::VisualConfigure {
                return Err(error(RequestErrorKind::PoseOnCommand(file.command)));
            }
            Some(
                visual::PoseRequest::try_from(table)
                    .map_err(|reason| error(RequestErrorKind::InvalidPose(reason)))?,
            )
        }
        None => None,
    };
    let anchor = match file.anchor {
        Some(table) => {
            if file.command != Command::VisualConfigure {
                return Err(error(RequestErrorKind::AnchorOnCommand(file.command)));
            }
            Some(Box::new(visual::AnchorPin::try_from(table).map_err(
                |reason| error(RequestErrorKind::InvalidPose(reason)),
            )?))
        }
        None => None,
    };
    let motion = match file.motion {
        Some(table) => {
            if file.command != Command::VisualConfigure {
                return Err(error(RequestErrorKind::MotionOnCommand(file.command)));
            }
            Some(
                visual::MotionRequest::try_from(table)
                    .map_err(|reason| error(RequestErrorKind::InvalidMotion(reason)))?,
            )
        }
        None => None,
    };
    if pose.is_some() && motion.is_some() {
        return Err(error(RequestErrorKind::MotionWithPose));
    }
    let environment = match file.environment {
        Some(table) => {
            if file.command != Command::VisualConfigure {
                return Err(error(RequestErrorKind::EnvironmentOnCommand(file.command)));
            }
            Some(
                visual::EnvironmentRequest::try_from(table)
                    .map_err(|e| error(RequestErrorKind::InvalidEnvironment(e)))?,
            )
        }
        None => None,
    };
    if file.stage.is_some() && file.command != Command::VisualStage {
        return Err(error(RequestErrorKind::StageOnCommand(file.command)));
    }
    if file.shader_identity.is_some() && file.command != Command::VisualConfigure {
        return Err(error(RequestErrorKind::ShaderIdentityOnCommand(
            file.command,
        )));
    }
    let visual_overrides = match file.visual_overrides {
        Some(table) => {
            if file.command != Command::VisualConfigure {
                return Err(error(RequestErrorKind::VisualOverridesOnCommand(
                    file.command,
                )));
            }
            Some(
                visual::VisualOverrides::try_from(table)
                    .map_err(|e| error(RequestErrorKind::InvalidVisualOverrides(e)))?,
            )
        }
        None => None,
    };
    if file.snapshot.is_some() && file.command != Command::Snapshot {
        return Err(error(RequestErrorKind::SnapshotOnCommand(file.command)));
    }
    if file.benchmark.is_some() && file.command != Command::Benchmark {
        return Err(error(RequestErrorKind::BenchmarkOnCommand(file.command)));
    }
    let motion_capture = match file.motion_capture {
        Some(table) => {
            if !matches!(file.command, Command::Screenshot | Command::Snapshot) {
                return Err(error(RequestErrorKind::MotionCaptureOnCommand(
                    file.command,
                )));
            }
            Some(table.phase)
        }
        None => None,
    };
    if file.water_reflection.is_some() && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::WaterReflectionOnCommand(
            file.command,
        )));
    }
    if file.water_reflection.is_some() && !file.water_inputs {
        return Err(error(RequestErrorKind::WaterReflectionNeedsInputs));
    }
    if (file.aa_color_edge.is_some() && (!file.aa_weights || file.patch63_hull != Some(true)))
        || (file.aa_weights && !file.aa_draws)
        || (file.aa_draws && !file.dof_inputs)
        || (file.lake_owner_four && (!file.lake_producer || !file.tess_writer))
        || (file.lake_producer && !file.lake_displacement)
        || (file.lake_gradients && !file.lake_waves)
        || (file.lake_waves && (!file.lake_displacement || file.lake_normals))
        || (file.lake_normals && !file.lake_displacement)
        || (file.lake_displacement
            && (!file.lake_samplers
                || !file.tess_writer
                || !matches!(
                    file.water_reflection,
                    Some(
                        crate::screenshot::harness::WaterReflection::LakeCameraControl
                            | crate::screenshot::harness::WaterReflection::LakeHullCamera
                    )
                )))
        || (file.patch63_hull.is_some()
            && (file.owner_four_domain != Some(true) || file.writer_domain_camera != Some(true)))
        || (file.owner_four_domain.is_some()
            && (!file.lake_owner_four || file.lake_color_mirror != Some(true)))
        || (file.lake_color_mirror.is_some() && file.lake_depth_edge != Some(true))
        || (file.lake_depth_edge.is_some()
            && (file.lake_ds_mvp != Some(true) || file.lake_waves || file.lake_normals))
        || (file.lake_ds_mvp.is_some()
            && (!file.lake_producer
                || file.card_atlas != Some(true)
                || file.water_reflection
                    != Some(crate::screenshot::harness::WaterReflection::LakeHullCamera)))
        || (file.card_atlas.is_some() && file.group_fade != Some(true))
        || (file.group_fade.is_some() && (!file.group_cards || file.lake_inner_edge != Some(true)))
        || (file.group_cards && !file.group_residual)
        || (file.group_residual && !file.group_inputs)
        || (file.lake_samplers && (!file.water_surface_inputs || file.water_tess_draw != Some(7)))
        || (file.tess_writer
            && (!file.terrain_sources || !file.tess_owners || file.terrain_camera.is_some()))
        || (file.tess_owners && !file.terrain_interval)
        || (file.base_projection && !file.base_sources)
        || (file.base_sources && !file.terrain_interval)
        || (file.terrain_interval && !file.post_geometry)
        || (file.group_inputs && !file.group_sources)
        || (file.group_sources && !file.left_interval)
        || (file.resolve_shadow && !file.resolve_samples)
        || (file.dof_inputs && !file.fog_generation)
        || (file.fog_inputs && !file.late_sky_stages)
        || (file.fog_generation && !file.fog_inputs)
        || (file.dof_edge_local.is_some() && !file.dof_inputs)
        || (file.lake_inner_edge.is_some()
            && (!file.lake_samplers || file.writer_domain_camera != Some(true)))
        || (file.writer_domain_camera.is_some()
            && (!file.tess_writer || file.base_eye_vp != Some(true)))
        || (file.base_eye_vp.is_some()
            && (!file.base_projection || file.group_atlas_scale != Some(true)))
        || (file.group_atlas_scale.is_some()
            && (!file.group_inputs
                || file.dof_edge_local != Some(true)
                || file.left_atlas_scale != Some(true)))
        || (file.fog_local_uv.is_some() && !file.fog_generation)
    {
        return Err(error(RequestErrorKind::InvalidLateSkyStages));
    }
    if file.late_sky_stages && (file.command != Command::Screenshot || motion_capture.is_some()) {
        return Err(error(RequestErrorKind::InvalidLateSkyStages));
    }
    if file.draw_census && (file.command != Command::Screenshot || motion_capture.is_some()) {
        return Err(error(RequestErrorKind::InvalidDrawCensus));
    }
    if file
        .second_patch_ordinal
        .is_some_and(|second| !file.patch_draw_ordinal.is_some_and(|first| first < second))
    {
        return Err(error(RequestErrorKind::InvalidPatchOrdinal));
    }
    if file.patch_draw_ordinal.is_some_and(|n| {
        n == 0
            || !matches!(
                file.water_reflection,
                Some(
                    crate::screenshot::harness::WaterReflection::LakeCameraControl
                        | crate::screenshot::harness::WaterReflection::LakeDomainCamera
                        | crate::screenshot::harness::WaterReflection::LakeHullCamera
                )
            )
    }) {
        return Err(error(RequestErrorKind::InvalidPatchOrdinal));
    }
    if file.water_surface_inputs
        && (file.water_tess_draw.is_none()
            || file.water_reflection.is_some_and(|r| {
                !matches!(
                    r,
                    crate::screenshot::harness::WaterReflection::EyeWidthPlanar
                        | crate::screenshot::harness::WaterReflection::LakeSamples
                        | crate::screenshot::harness::WaterReflection::LakeLighting
                        | crate::screenshot::harness::WaterReflection::LakeSky
                        | crate::screenshot::harness::WaterReflection::LakeSkyBuffer
                        | crate::screenshot::harness::WaterReflection::LakeResponse
                        | crate::screenshot::harness::WaterReflection::LakeNearPatch
                        | crate::screenshot::harness::WaterReflection::LakeTessPatch
                        | crate::screenshot::harness::WaterReflection::LakePatchDraw
                        | crate::screenshot::harness::WaterReflection::LakeCameraControl
                        | crate::screenshot::harness::WaterReflection::LakePatchOwners
                        | crate::screenshot::harness::WaterReflection::LakeDomainCamera
                        | crate::screenshot::harness::WaterReflection::LakeHullCamera
                )
            })
            || motion_capture.is_some())
    {
        return Err(error(RequestErrorKind::InvalidSurfaceInputs));
    }
    if file
        .water_tess_draw
        .is_some_and(|n| !file.water_inputs || n >= 32)
    {
        return Err(error(RequestErrorKind::InvalidWaterDraw));
    }
    if file.water_inputs && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::WaterInputsOnCommand(file.command)));
    }
    if file.sun_inputs && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::SunInputsOnCommand(file.command)));
    }
    if file.history_center.is_some() && (!file.ssr_history || file.veto_eye != Some(true)) {
        return Err(error(RequestErrorKind::InvalidResolveSamples));
    }
    if file.grid_eye.is_some() && file.veto_eye != Some(true) {
        return Err(error(RequestErrorKind::InvalidResolveSamples));
    }
    if file.veto_eye.is_some() && file.march_eye != Some(true) {
        return Err(error(RequestErrorKind::InvalidResolveSamples));
    }
    if file.march_eye.is_some() && file.proxy_eye != Some(true) {
        return Err(error(RequestErrorKind::InvalidResolveSamples));
    }
    if file.proxy_eye.is_some() && (!file.ssr_prepare || file.resolve_eye_camera != Some(true)) {
        return Err(error(RequestErrorKind::InvalidResolveSamples));
    }
    if (file.extra_bark_dither.is_some()
        && (!file.residual_bark || file.left_atlas_scale != Some(true)))
        || (file.terrain_camera.is_some()
            && (!file.terrain_sources || file.extra_bark_dither != Some(true)))
        || (file.terrain_sources && !file.early_depth)
        || (file.residual_bark && !file.left_interval)
        || (file.left_atlas_scale.is_some()
            && (!file.left_interval || file.impostor_scale != Some(true)))
        || (file.left_interval && !file.impostor_inputs)
        || (file.impostor_scale.is_some()
            && (!file.impostor_inputs || file.color_dither != Some(true)))
        || (file.impostor_inputs && !file.interval_trace)
        || (file.interval_trace && !file.post_geometry)
        || (file.post_geometry && !file.bark_color_trace)
        || (file.color_dither.is_some()
            && (!file.bark_color_trace || file.bark_dither != Some(true)))
        || ((file.bark_dither.is_some() || file.bark_color_trace) && !file.velocity_trace)
        || (file.velocity_trace && !file.early_depth)
    {
        return Err(error(RequestErrorKind::InvalidPatchOrdinal));
    }
    if file.early_depth && (file.patch_draw_ordinal.is_none() || !file.ssr_history) {
        return Err(error(RequestErrorKind::InvalidPatchOrdinal));
    }
    if file.ssr_history && !file.ssr_prepare {
        return Err(error(RequestErrorKind::InvalidResolveSamples));
    }
    if (file.ssr_prepare && !file.ssr_operands) || (file.ssr_operands && !file.ssr_bracket) {
        return Err(error(RequestErrorKind::InvalidResolveSamples));
    }
    if (file.resolve_eye_camera.is_some() || file.ssr_bracket) && !file.resolve_samples {
        return Err(error(RequestErrorKind::InvalidResolveSamples));
    }
    if file.resolve_samples
        && (!file.resolve_inputs
            || file.cascade_scale.is_some()
            || motion_capture.is_some()
            || file.command != Command::Screenshot)
    {
        return Err(error(RequestErrorKind::InvalidResolveSamples));
    }
    if file.cascade_scale.is_some()
        && (!file.resolve_inputs || file.command != Command::Screenshot || motion_capture.is_some())
    {
        return Err(error(RequestErrorKind::InvalidCascadeScale));
    }
    if file.resolve_inputs && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::ResolveInputsOnCommand(
            file.command,
        )));
    }
    if file.sun_depth_clip.is_some() && file.command != Command::Screenshot {
        return Err(error(RequestErrorKind::SunDepthClipOnCommand(file.command)));
    }
    if file.sun_depth_clip.is_some() && !file.sun_inputs {
        return Err(error(RequestErrorKind::SunDepthClipNeedsSunInputs));
    }
    Ok(match file.command {
        Command::VisualConfigure | Command::VisualEnd => {
            let id = visual::RequestId::parse(file.id.as_deref().unwrap_or_default())
                .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?;
            Request::Visual(if file.command == Command::VisualEnd {
                visual::Request::End { id }
            } else {
                let mode = file.mode.ok_or_else(|| error(RequestErrorKind::NoMode))?;
                if mode == visual::RenderMode::Native {
                    return Err(error(RequestErrorKind::InvalidMode));
                }
                let warmup_frames = file.warmup_frames.unwrap_or(30);
                if warmup_frames > 300 {
                    return Err(error(RequestErrorKind::InvalidWarmup));
                }
                if let Some(overrides) = &visual_overrides {
                    overrides
                        .validate_for(mode)
                        .map_err(|e| error(RequestErrorKind::InvalidVisualOverrides(e)))?;
                }
                visual::Request::Configure {
                    id,
                    mode,
                    warmup_frames,
                    pose,
                    anchor,
                    motion,
                    environment,
                    shader_identity: file.shader_identity.unwrap_or(false),
                    visual_overrides: visual_overrides.map(Box::new),
                }
            })
        }
        Command::VisualStage => {
            let id = visual::RequestId::parse(file.id.as_deref().unwrap_or_default())
                .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?;
            let table = file.stage.ok_or_else(|| error(RequestErrorKind::NoStage))?;
            Request::Stage(
                StageRequest::new(id, table)
                    .map_err(|e| error(RequestErrorKind::InvalidStage(e)))?,
            )
        }
        Command::Snapshot => {
            let id = crate::screenshot::harness::RequestId::parse(
                file.id.as_deref().unwrap_or_default(),
            )
            .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?;
            let table = file
                .snapshot
                .ok_or_else(|| error(RequestErrorKind::NoSnapshot))?;
            Request::Snapshot(
                SnapshotRequest::parse(id, motion_capture, table)
                    .map_err(|e| error(RequestErrorKind::InvalidSnapshot(e)))?,
            )
        }
        Command::Benchmark => {
            let id = crate::screenshot::harness::RequestId::parse(
                file.id.as_deref().unwrap_or_default(),
            )
            .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?;
            let table = file
                .benchmark
                .ok_or_else(|| error(RequestErrorKind::NoBenchmark))?;
            Request::Benchmark(
                BenchmarkRequest::parse(id, table)
                    .map_err(|e| error(RequestErrorKind::InvalidBenchmark(e)))?,
            )
        }
        Command::SnapshotRelease => Request::SnapshotRelease(
            crate::screenshot::harness::RequestId::parse(file.id.as_deref().unwrap_or_default())
                .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?,
        ),
        Command::Screenshot => Request::Screenshot {
            id: crate::screenshot::harness::RequestId::parse(
                file.id.as_deref().unwrap_or_default(),
            )
            .map_err(|reason| error(RequestErrorKind::InvalidId(reason)))?,
            motion: motion_capture,
            water_reflection: file.water_reflection,
            water_inputs: file.water_inputs,
            water_tess_draw: file.water_tess_draw,
            patch_draw_ordinal: file.patch_draw_ordinal,
            second_patch_ordinal: file.second_patch_ordinal,
            water_surface_inputs: file.water_surface_inputs,
            late_sky_stages: file.late_sky_stages,
            draw_census: file.draw_census,
            fog_inputs: file.fog_inputs,
            group_sources: file.group_sources,
            group_inputs: file.group_inputs,
            terrain_interval: file.terrain_interval,
            base_sources: file.base_sources,
            base_projection: file.base_projection,
            group_cards: file.group_cards,
            group_residual: file.group_residual,
            aa_weights: file.aa_weights,
            aa_draws: file.aa_draws,
            lake_owner_four: file.lake_owner_four,
            lake_producer: file.lake_producer,
            lake_gradients: file.lake_gradients,
            lake_waves: file.lake_waves,
            lake_normals: file.lake_normals,
            lake_displacement: file.lake_displacement,
            lake_samplers: file.lake_samplers,
            tess_writer: file.tess_writer,
            tess_owners: file.tess_owners,
            resolve_shadow: file.resolve_shadow,
            dof_inputs: file.dof_inputs,
            fog_generation: file.fog_generation,
            sun_inputs: file.sun_inputs,
            resolve_inputs: file.resolve_inputs,
            resolve_samples: file.resolve_samples,
            ssr_bracket: file.ssr_bracket,
            ssr_operands: file.ssr_operands,
            ssr_prepare: file.ssr_prepare,
            ssr_history: file.ssr_history,
            dof_edge_local: file.dof_edge_local,
            card_atlas: file.card_atlas,
            group_fade: file.group_fade,
            group_atlas_scale: file.group_atlas_scale,
            aa_color_edge: file.aa_color_edge,
            patch63_hull: file.patch63_hull,
            owner_four_domain: file.owner_four_domain,
            lake_color_mirror: file.lake_color_mirror,
            lake_depth_edge: file.lake_depth_edge,
            lake_ds_mvp: file.lake_ds_mvp,
            lake_inner_edge: file.lake_inner_edge,
            writer_domain_camera: file.writer_domain_camera,
            base_eye_vp: file.base_eye_vp,
            fog_local_uv: file.fog_local_uv,
            terrain_camera: file.terrain_camera,
            extra_bark_dither: file.extra_bark_dither,
            left_atlas_scale: file.left_atlas_scale,
            impostor_scale: file.impostor_scale,
            color_dither: file.color_dither,
            bark_dither: file.bark_dither,
            terrain_sources: file.terrain_sources,
            residual_bark: file.residual_bark,
            left_interval: file.left_interval,
            impostor_inputs: file.impostor_inputs,
            interval_trace: file.interval_trace,
            post_geometry: file.post_geometry,
            bark_color_trace: file.bark_color_trace,
            velocity_trace: file.velocity_trace,
            early_depth: file.early_depth,
            resolve_eye_camera: file.resolve_eye_camera,
            proxy_eye: file.proxy_eye,
            march_eye: file.march_eye,
            veto_eye: file.veto_eye,
            grid_eye: file.grid_eye,
            history_center: file.history_center,
            cascade_scale: file.cascade_scale,
            sun_depth_clip: file.sun_depth_clip,
        },
        Command::Record => Request::Record,
        Command::StopRecording => Request::StopRecording,
        Command::Replay => Request::Replay {
            path: resolve(Command::Replay)?,
            options: ReplayOptions {
                start: file.start,
                pacing: file.pacing,
            },
        },
        Command::StopReplay => Request::StopReplay,
    })
}

fn act(request: Request) {
    tracing::info!(?request, "testing: request file");
    match request {
        Request::Visual(request) => visual::enqueue(request),
        Request::Stage(request) => stage::enqueue(request),
        Request::Snapshot(request) => crate::screenshot::harness::snapshot::request(request),
        Request::SnapshotRelease(id) => crate::screenshot::harness::snapshot::release(id),
        Request::Benchmark(request) => crate::testing::benchmark::request(request),
        Request::Screenshot {
            id,
            motion,
            water_reflection,
            water_inputs,
            water_tess_draw,
            patch_draw_ordinal,
            second_patch_ordinal,
            water_surface_inputs,
            late_sky_stages,
            draw_census,
            fog_inputs,
            group_sources,
            group_inputs,
            terrain_interval,
            base_sources,
            base_projection,
            group_cards,
            group_residual,
            aa_weights,
            aa_draws,
            lake_owner_four,
            lake_producer,
            lake_gradients,
            lake_waves,
            lake_normals,
            lake_displacement,
            lake_samplers,
            tess_writer,
            tess_owners,
            resolve_shadow,
            dof_inputs,
            fog_generation,
            sun_inputs,
            resolve_inputs,
            ssr_bracket,
            ssr_operands,
            ssr_prepare,
            ssr_history,
            dof_edge_local,
            card_atlas,
            group_fade,
            group_atlas_scale,
            aa_color_edge,
            patch63_hull,
            owner_four_domain,
            lake_color_mirror,
            lake_depth_edge,
            lake_ds_mvp,
            lake_inner_edge,
            writer_domain_camera,
            base_eye_vp,
            fog_local_uv,
            terrain_camera,
            extra_bark_dither,
            left_atlas_scale,
            impostor_scale,
            color_dither,
            bark_dither,
            terrain_sources,
            residual_bark,
            left_interval,
            impostor_inputs,
            interval_trace,
            post_geometry,
            bark_color_trace,
            velocity_trace,
            early_depth,
            resolve_samples,
            resolve_eye_camera,
            proxy_eye,
            march_eye,
            veto_eye,
            grid_eye,
            history_center,
            cascade_scale,
            sun_depth_clip,
        } => {
            let options = crate::screenshot::harness::CaptureRequest {
                water_reflection,
                water_inputs,
                water_tess_draw,
                patch_draw_ordinal,
                second_patch_ordinal,
                water_surface_inputs,
                late_sky_stages,
                draw_census,
                fog_inputs,
                group_sources,
                group_inputs,
                terrain_interval,
                base_sources,
                base_projection,
                group_cards,
                group_residual,
                aa_weights,
                aa_draws,
                lake_owner_four,
                lake_producer,
                lake_gradients,
                lake_waves,
                lake_normals,
                lake_displacement,
                lake_samplers,
                tess_writer,
                tess_owners,
                resolve_shadow,
                dof_inputs,
                fog_generation,
                sun_inputs,
                resolve_inputs,
                ssr_bracket,
                ssr_operands,
                ssr_prepare,
                ssr_history,
                dof_edge_local,
                card_atlas,
                group_fade,
                group_atlas_scale,
                aa_color_edge,
                patch63_hull,
                owner_four_domain,
                lake_color_mirror,
                lake_depth_edge,
                lake_ds_mvp,
                lake_inner_edge,
                writer_domain_camera,
                base_eye_vp,
                fog_local_uv,
                terrain_camera,
                extra_bark_dither,
                left_atlas_scale,
                impostor_scale,
                color_dither,
                bark_dither,
                terrain_sources,
                residual_bark,
                left_interval,
                impostor_inputs,
                interval_trace,
                post_geometry,
                bark_color_trace,
                velocity_trace,
                early_depth,
                resolve_samples,
                resolve_eye_camera,
                proxy_eye,
                march_eye,
                veto_eye,
                grid_eye,
                history_center,
                cascade_scale,
                sun_depth_clip,
            };
            match motion {
                Some(phase) => crate::screenshot::harness::request_motion(id, phase, options),
                None => crate::screenshot::harness::request(id, options),
            }
        }
        Request::Record => crate::recorder::tracking::request_start(),
        Request::StopRecording => crate::recorder::tracking::request_stop(),
        Request::Replay { path, options } => {
            replay::start(path, options);
        }
        Request::StopReplay => replay::stop(),
    }
}

#[cfg(test)]
mod tests;
