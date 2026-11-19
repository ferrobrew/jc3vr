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
pub(crate) mod draw_census;
pub(crate) mod patch_draw;
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
    PlanarUv,
    LakeSamples,
    LakeLighting,
    LakeSky,
    LakeSkyBuffer,
    LakeResponse,
    LakeNearPatch,
    LakeTessPatch,
    LakePatchDraw,
    LakePatchOwners,
    LakeCameraControl,
    LakeHullCamera,
    LakeDomainCamera,
    EyeWidthPlanar,
    MirroredPlanar,
    ProjectedPlanar,
    PristineLake,
    ZeroWeight,
    ZeroPlanar,
    CenterPlanar,
    PackedRefraction,
}

impl WaterReflection {
    fn supported(self, mode: RenderMode, selection: Option<u8>) -> bool {
        match self {
            Self::ZeroWeight => mode == RenderMode::Multipass,
            Self::PlanarUv
            | Self::LakeSamples
            | Self::LakeLighting
            | Self::LakeResponse
            | Self::LakeNearPatch
            | Self::LakeTessPatch
            | Self::LakePatchDraw
            | Self::LakePatchOwners
            | Self::LakeCameraControl => {
                selection == Some(7)
                    && matches!(
                        mode,
                        RenderMode::LeftOnly
                            | RenderMode::RightOnly
                            | RenderMode::Multipass
                            | RenderMode::SinglePass
                    )
            }
            Self::ZeroPlanar => {
                selection.is_some()
                    && matches!(
                        mode,
                        RenderMode::LeftOnly
                            | RenderMode::RightOnly
                            | RenderMode::Multipass
                            | RenderMode::SinglePass
                    )
            }
            Self::LakeHullCamera
            | Self::LakeDomainCamera
            | Self::EyeWidthPlanar
            | Self::LakeSky
            | Self::LakeSkyBuffer => mode == RenderMode::SinglePass && selection == Some(7),
            Self::MirroredPlanar
            | Self::ProjectedPlanar
            | Self::PristineLake
            | Self::PackedRefraction
            | Self::CenterPlanar => mode == RenderMode::SinglePass && selection.is_some(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SunDepthClip {
    DisabledRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CascadeScale {
    ShoreLow,
    ShoreHigh,
}

impl CascadeScale {
    pub(crate) fn bits(self) -> u32 {
        match self {
            Self::ShoreLow => 0x3c07e14c,
            Self::ShoreHigh => 0x3c07ec65,
        }
    }
}

pub(crate) struct CaptureRequest {
    pub water_reflection: Option<WaterReflection>,
    pub water_inputs: bool,
    pub water_tess_draw: Option<u8>,
    pub patch_draw_ordinal: Option<u8>,
    pub second_patch_ordinal: Option<u8>,
    pub water_surface_inputs: bool,
    pub late_sky_stages: bool,
    pub draw_census: bool,
    pub fog_inputs: bool,
    pub group_sources: bool,
    pub group_inputs: bool,
    pub terrain_interval: bool,
    pub base_sources: bool,
    pub base_projection: bool,
    pub group_cards: bool,
    pub group_residual: bool,
    pub aa_weights: bool,
    pub aa_draws: bool,
    pub lake_owner_four: bool,
    pub lake_producer: bool,
    pub lake_gradients: bool,
    pub lake_waves: bool,
    pub lake_normals: bool,
    pub lake_displacement: bool,
    pub lake_samplers: bool,
    pub tess_writer: bool,
    pub tess_owners: bool,
    pub resolve_shadow: bool,
    pub dof_inputs: bool,
    pub fog_generation: bool,
    pub sun_inputs: bool,
    pub resolve_inputs: bool,
    pub ssr_bracket: bool,
    pub ssr_operands: bool,
    pub ssr_prepare: bool,
    pub ssr_history: bool,
    pub dof_edge_local: Option<bool>,
    pub card_atlas: Option<bool>,
    pub group_fade: Option<bool>,
    pub group_atlas_scale: Option<bool>,
    pub aa_color_edge: Option<bool>,
    pub patch63_hull: Option<bool>,
    pub owner_four_domain: Option<bool>,
    pub lake_color_mirror: Option<bool>,
    pub lake_depth_edge: Option<bool>,
    pub lake_ds_mvp: Option<bool>,
    pub lake_inner_edge: Option<bool>,
    pub writer_domain_camera: Option<bool>,
    pub base_eye_vp: Option<bool>,
    pub fog_local_uv: Option<bool>,
    pub terrain_camera: Option<bool>,
    pub extra_bark_dither: Option<bool>,
    pub left_atlas_scale: Option<bool>,
    pub impostor_scale: Option<bool>,
    pub color_dither: Option<bool>,
    pub bark_dither: Option<bool>,
    pub terrain_sources: bool,
    pub residual_bark: bool,
    pub left_interval: bool,
    pub impostor_inputs: bool,
    pub interval_trace: bool,
    pub post_geometry: bool,
    pub bark_color_trace: bool,
    pub velocity_trace: bool,
    pub early_depth: bool,
    pub resolve_samples: bool,
    pub resolve_eye_camera: Option<bool>,
    pub proxy_eye: Option<bool>,
    pub march_eye: Option<bool>,
    pub veto_eye: Option<bool>,
    pub grid_eye: Option<bool>,
    pub history_center: Option<bool>,
    pub cascade_scale: Option<CascadeScale>,
    pub sun_depth_clip: Option<SunDepthClip>,
}

pub(crate) fn request(id: RequestId, options: CaptureRequest) {
    if (options.aa_color_edge.is_some()
        && (!options.aa_weights || options.patch63_hull != Some(true)))
        || (options.aa_weights && !options.aa_draws)
        || (options.aa_draws && !options.dof_inputs)
        || (options.lake_owner_four && (!options.lake_producer || !options.tess_writer))
        || (options.lake_producer && !options.lake_displacement)
        || (options.lake_gradients && !options.lake_waves)
        || (options.lake_waves && (!options.lake_displacement || options.lake_normals))
        || (options.lake_normals && !options.lake_displacement)
        || (options.lake_displacement
            && (!options.lake_samplers
                || !options.tess_writer
                || !matches!(
                    options.water_reflection,
                    Some(WaterReflection::LakeCameraControl | WaterReflection::LakeHullCamera)
                )))
        || (options.patch63_hull.is_some()
            && (options.owner_four_domain != Some(true)
                || options.writer_domain_camera != Some(true)))
        || (options.owner_four_domain.is_some()
            && (!options.lake_owner_four || options.lake_color_mirror != Some(true)))
        || (options.lake_color_mirror.is_some() && options.lake_depth_edge != Some(true))
        || (options.lake_depth_edge.is_some()
            && (options.lake_ds_mvp != Some(true) || options.lake_waves || options.lake_normals))
        || (options.lake_ds_mvp.is_some()
            && (!options.lake_producer
                || options.card_atlas != Some(true)
                || options.water_reflection
                    != Some(crate::screenshot::harness::WaterReflection::LakeHullCamera)))
        || (options.card_atlas.is_some() && options.group_fade != Some(true))
        || (options.group_fade.is_some()
            && (!options.group_cards || options.lake_inner_edge != Some(true)))
        || (options.group_cards && !options.group_residual)
        || (options.group_residual && !options.group_inputs)
        || (options.lake_samplers
            && (!options.water_surface_inputs || options.water_tess_draw != Some(7)))
        || (options.tess_writer
            && (!options.terrain_sources
                || !options.tess_owners
                || options.terrain_camera.is_some()))
        || (options.tess_owners && !options.terrain_interval)
        || (options.base_projection && !options.base_sources)
        || (options.base_sources && !options.terrain_interval)
        || (options.terrain_interval && !options.post_geometry)
        || (options.group_inputs && !options.group_sources)
        || (options.group_sources && !options.left_interval)
        || (options.resolve_shadow && !options.resolve_samples)
        || (options.dof_inputs && !options.fog_generation)
        || (options.fog_generation && !options.fog_inputs)
        || (options.dof_edge_local.is_some() && !options.dof_inputs)
        || (options.lake_inner_edge.is_some()
            && (!options.lake_samplers || options.writer_domain_camera != Some(true)))
        || (options.writer_domain_camera.is_some()
            && (!options.tess_writer || options.base_eye_vp != Some(true)))
        || (options.base_eye_vp.is_some()
            && (!options.base_projection || options.group_atlas_scale != Some(true)))
        || (options.group_atlas_scale.is_some()
            && (!options.group_inputs
                || options.dof_edge_local != Some(true)
                || options.left_atlas_scale != Some(true)))
        || (options.fog_local_uv.is_some() && !options.fog_generation)
        || (options.fog_inputs && !options.late_sky_stages)
        || (options.extra_bark_dither.is_some()
            && (!options.residual_bark || options.left_atlas_scale != Some(true)))
        || (options.terrain_camera.is_some()
            && (!options.terrain_sources || options.extra_bark_dither != Some(true)))
        || (options.terrain_sources && !options.early_depth)
        || (options.residual_bark && !options.left_interval)
        || (options.left_atlas_scale.is_some()
            && (!options.left_interval || options.impostor_scale != Some(true)))
        || (options.left_interval && !options.impostor_inputs)
        || (options.impostor_scale.is_some()
            && (!options.impostor_inputs || options.color_dither != Some(true)))
        || (options.impostor_inputs && !options.interval_trace)
        || (options.interval_trace && !options.post_geometry)
        || (options.post_geometry && !options.bark_color_trace)
        || (options.color_dither.is_some()
            && (!options.bark_color_trace || options.bark_dither != Some(true)))
        || ((options.bark_dither.is_some() || options.bark_color_trace) && !options.velocity_trace)
        || (options.velocity_trace && !options.early_depth)
    {
        reject(&id, "velocity trace: early depth bracket required");
        return;
    }
    if options.early_depth && (options.patch_draw_ordinal.is_none() || !options.ssr_history) {
        reject(
            &id,
            "early depth: missing selected terrain draw or history trace",
        );
        return;
    }
    if options.ssr_history && !options.ssr_prepare {
        reject(&id, "SSR history: preparation capture required");
        return;
    }
    if options.history_center.is_some() && (!options.ssr_history || options.veto_eye != Some(true))
    {
        reject(
            &id,
            "SSR history center: traced history and packed veto required",
        );
        return;
    }
    if options.grid_eye.is_some() && options.veto_eye != Some(true) {
        reject(&id, "SSR grid: packed veto required");
        return;
    }
    if options.veto_eye.is_some() && options.march_eye != Some(true) {
        reject(&id, "SSR veto: packed march required");
        return;
    }
    if options.march_eye.is_some() && options.proxy_eye != Some(true) {
        reject(&id, "march trial requires the physical proxy");
        return;
    }
    if options.proxy_eye.is_some()
        && (options.resolve_eye_camera != Some(true) || !options.ssr_prepare)
    {
        reject(
            &id,
            "proxy trial requires a physical camera and preparation capture",
        );
        return;
    }
    if options.resolve_eye_camera.is_some()
        && (dispatch::mode() != RenderMode::SinglePass
            || !options.resolve_samples
            || !options.resolve_inputs
            || options.cascade_scale.is_some()
            || options.water_reflection != Some(WaterReflection::LakeHullCamera))
    {
        reject(
            &id,
            "resolve camera: requires SPS lake hull camera and resolve samples",
        );
        return;
    }
    if options.cascade_scale.is_some()
        && (dispatch::mode() != RenderMode::Multipass
            || !options.resolve_inputs
            || crate::stereo::single_pass::shader_identity::status().status != "complete")
    {
        reject(
            &id,
            "cascade treatment: multipass, resolve inputs and complete census required",
        );
        return;
    }
    if let Some(treatment) = options.water_reflection {
        if !treatment.supported(dispatch::mode(), options.water_tess_draw)
            || (matches!(
                treatment,
                WaterReflection::EyeWidthPlanar
                    | WaterReflection::LakeSamples
                    | WaterReflection::LakeLighting
                    | WaterReflection::LakeSky
                    | WaterReflection::LakeSkyBuffer
                    | WaterReflection::LakeResponse
                    | WaterReflection::LakeNearPatch
                    | WaterReflection::LakeTessPatch
                    | WaterReflection::LakePatchDraw
                    | WaterReflection::LakeCameraControl
                    | WaterReflection::LakePatchOwners
                    | WaterReflection::LakeDomainCamera
                    | WaterReflection::LakeHullCamera
            ) && !options.water_surface_inputs)
        {
            reject(
                &id,
                "water reflection treatment: incompatible mode or draw selection",
            );
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
        active.water_tess_draw = options.water_tess_draw;
        active.patch_draw_ordinal = options.patch_draw_ordinal;
        active.second_patch_ordinal = options.second_patch_ordinal;
        active.water_surface_inputs = options.water_surface_inputs;
        active.late_sky_stages = options.late_sky_stages;
        active.draw_census = options.draw_census;
        active.fog_inputs = options.fog_inputs;
        active.group_sources = options.group_sources;
        active.group_inputs = options.group_inputs;
        active.terrain_interval = options.terrain_interval;
        active.base_sources = options.base_sources;
        active.base_projection = options.base_projection;
        active.group_cards = options.group_cards;
        active.group_residual = options.group_residual;
        active.aa_weights = options.aa_weights;
        active.aa_draws = options.aa_draws;
        active.lake_owner_four = options.lake_owner_four;
        active.lake_producer = options.lake_producer;
        active.lake_gradients = options.lake_gradients;
        active.lake_waves = options.lake_waves;
        active.lake_normals = options.lake_normals;
        active.lake_displacement = options.lake_displacement;
        active.lake_samplers = options.lake_samplers;
        active.tess_writer = options.tess_writer;
        active.tess_owners = options.tess_owners;
        active.resolve_shadow = options.resolve_shadow;
        active.dof_inputs = options.dof_inputs;
        active.fog_generation = options.fog_generation;
        active.sun_inputs = options.sun_inputs;
        active.resolve_inputs = options.resolve_inputs;
        active.ssr_bracket = options.ssr_bracket;
        active.ssr_operands = options.ssr_operands;
        active.ssr_prepare = options.ssr_prepare;
        active.ssr_history = options.ssr_history;
        active.dof_edge_local = options.dof_edge_local;
        active.card_atlas = options.card_atlas;
        active.group_fade = options.group_fade;
        active.group_atlas_scale = options.group_atlas_scale;
        active.aa_color_edge = options.aa_color_edge;
        active.patch63_hull = options.patch63_hull;
        active.owner_four_domain = options.owner_four_domain;
        active.lake_color_mirror = options.lake_color_mirror;
        active.lake_depth_edge = options.lake_depth_edge;
        active.lake_ds_mvp = options.lake_ds_mvp;
        active.lake_inner_edge = options.lake_inner_edge;
        active.writer_domain_camera = options.writer_domain_camera;
        active.base_eye_vp = options.base_eye_vp;
        active.fog_local_uv = options.fog_local_uv;
        active.terrain_camera = options.terrain_camera;
        active.extra_bark_dither = options.extra_bark_dither;
        active.left_atlas_scale = options.left_atlas_scale;
        active.impostor_scale = options.impostor_scale;
        active.color_dither = options.color_dither;
        active.bark_dither = options.bark_dither;
        active.terrain_sources = options.terrain_sources;
        active.residual_bark = options.residual_bark;
        active.left_interval = options.left_interval;
        active.impostor_inputs = options.impostor_inputs;
        active.interval_trace = options.interval_trace;
        active.post_geometry = options.post_geometry;
        active.bark_color_trace = options.bark_color_trace;
        active.velocity_trace = options.velocity_trace;
        active.early_depth = options.early_depth;
        active.resolve_samples = options.resolve_samples;
        active.resolve_eye_camera = options.resolve_eye_camera;
        active.proxy_eye = options.proxy_eye;
        active.march_eye = options.march_eye;
        active.veto_eye = options.veto_eye;
        active.grid_eye = options.grid_eye;
        active.history_center = options.history_center;
        active.cascade_scale = options.cascade_scale;
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
        water_tess_draw: None,
        patch_draw_ordinal: None,
        second_patch_ordinal: None,
        water_surface_inputs: false,
        late_sky_stages: false,
        draw_census: false,
        fog_inputs: false,
        group_sources: false,
        group_inputs: false,
        terrain_interval: false,
        base_sources: false,
        base_projection: false,
        group_cards: false,
        group_residual: false,
        aa_weights: false,
        aa_draws: false,
        lake_owner_four: false,
        lake_producer: false,
        lake_gradients: false,
        lake_waves: false,
        lake_normals: false,
        lake_displacement: false,
        lake_samplers: false,
        tess_writer: false,
        tess_owners: false,
        resolve_shadow: false,
        dof_inputs: false,
        fog_generation: false,
        sun_inputs: false,
        resolve_inputs: false,
        ssr_bracket: false,
        ssr_operands: false,
        ssr_prepare: false,
        ssr_history: false,
        dof_edge_local: None,
        card_atlas: None,
        group_fade: None,
        group_atlas_scale: None,
        aa_color_edge: None,
        patch63_hull: None,
        owner_four_domain: None,
        lake_color_mirror: None,
        lake_depth_edge: None,
        lake_ds_mvp: None,
        lake_inner_edge: None,
        writer_domain_camera: None,
        base_eye_vp: None,
        fog_local_uv: None,
        terrain_camera: None,
        extra_bark_dither: None,
        left_atlas_scale: None,
        impostor_scale: None,
        color_dither: None,
        bark_dither: None,
        terrain_sources: false,
        residual_bark: false,
        left_interval: false,
        impostor_inputs: false,
        interval_trace: false,
        post_geometry: false,
        bark_color_trace: false,
        velocity_trace: false,
        early_depth: false,
        resolve_samples: false,
        resolve_eye_camera: None,
        proxy_eye: None,
        march_eye: None,
        veto_eye: None,
        grid_eye: None,
        history_center: None,
        cascade_scale: None,
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

pub(crate) fn water_reflection_requested() -> Option<WaterReflection> {
    requested_water_diagnostic().map(|t| match t {
        WaterReflection::LakeNearPatch
        | WaterReflection::LakeTessPatch
        | WaterReflection::LakePatchDraw
        | WaterReflection::LakeCameraControl
        | WaterReflection::LakePatchOwners
        | WaterReflection::LakeDomainCamera
        | WaterReflection::LakeHullCamera => WaterReflection::LakeResponse,
        other => other,
    })
}

pub(crate) fn requested_water_diagnostic() -> Option<WaterReflection> {
    diagnostic_capture_id()?;
    let state = STATE.lock();
    let active = state.active.as_ref()?;
    active.water_reflection.filter(|t| {
        t.supported(dispatch::mode(), active.water_tess_draw)
            && matches!(active.phase, Phase::Pending)
    })
}

/// The frame a requested draw census records, while that frame is drawing.
pub(crate) fn draw_census_frame() -> Option<i64> {
    if !wants_eyes() || !STATE.lock().active.as_ref().is_some_and(|a| a.draw_census) {
        return None;
    }
    diagnostic_capture_id()?;
    frame_identity()
}

pub(crate) fn late_sky_stages_requested() -> bool {
    if !wants_eyes()
        || !STATE
            .lock()
            .active
            .as_ref()
            .is_some_and(|a| a.late_sky_stages)
    {
        return false;
    }
    diagnostic_capture_id().is_some()
}

pub(crate) fn water_surface_inputs_requested() -> bool {
    STATE
        .lock()
        .active
        .as_ref()
        .is_some_and(|a| a.water_surface_inputs && matches!(a.phase, Phase::Pending))
}

pub(crate) fn second_patch_ordinal() -> Option<u8> {
    STATE.lock().active.as_ref()?.second_patch_ordinal
}

pub(crate) fn patch_draw_ordinal() -> Option<u8> {
    STATE.lock().active.as_ref()?.patch_draw_ordinal
}

pub(crate) fn water_tess_draw() -> Option<u8> {
    STATE.lock().active.as_ref()?.water_tess_draw
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

pub(crate) fn cascade_scale(frame: i64) -> Option<CascadeScale> {
    if dispatch::mode() != RenderMode::Multipass {
        return None;
    }
    let state = STATE.lock();
    let active = state.active.as_ref()?;
    active.cascade_scale.filter(|_| {
        active.diagnostic_frame == Some(frame) && matches!(active.phase, Phase::Pending)
    })
}

pub(crate) fn proxy_eye(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.proxy_eye.filter(|_| {
        a.ssr_prepare && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn march_eye(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.march_eye.filter(|_| {
        a.ssr_prepare && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn veto_eye(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.veto_eye.filter(|_| {
        a.ssr_prepare && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn grid_eye(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.grid_eye.filter(|_| {
        a.ssr_prepare && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn history_center(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.history_center.filter(|_| {
        a.ssr_history && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn resolve_eye_camera(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.resolve_eye_camera.filter(|_| {
        a.resolve_inputs
            && a.resolve_samples
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn resolve_samples_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.resolve_samples
            && a.resolve_inputs
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn ssr_prepare_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.ssr_prepare
            && a.ssr_operands
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn ssr_history_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.ssr_history
            && a.ssr_operands
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn dof_edge_local(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.dof_edge_local.filter(|_| {
        a.dof_inputs && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn card_atlas(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.card_atlas.filter(|_| {
        a.group_cards && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_fade(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.group_fade.filter(|_| {
        a.group_cards && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_atlas_scale(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.group_atlas_scale.filter(|_| {
        a.group_inputs && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn aa_color_edge(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.aa_color_edge.filter(|_| {
        a.aa_weights && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn patch63_hull(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.patch63_hull.filter(|_| {
        a.lake_owner_four && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn owner_four_domain(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.owner_four_domain.filter(|_| {
        a.lake_owner_four && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_color_mirror(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.lake_color_mirror.filter(|_| {
        a.lake_producer && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_depth_edge(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.lake_depth_edge.filter(|_| {
        a.lake_producer && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_ds_mvp(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.lake_ds_mvp.filter(|_| {
        a.lake_producer && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_inner_edge(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.lake_inner_edge.filter(|_| {
        a.lake_samplers && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn writer_domain_camera(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.writer_domain_camera.filter(|_| {
        a.tess_writer && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn base_eye_vp(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.base_eye_vp.filter(|_| {
        a.base_projection && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn fog_local_uv(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.fog_local_uv.filter(|_| {
        a.fog_generation && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn terrain_camera(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.terrain_camera.filter(|_| {
        a.terrain_sources && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn extra_bark_dither(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.extra_bark_dither.filter(|_| {
        a.residual_bark && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn left_atlas_scale(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.left_atlas_scale.filter(|_| {
        a.left_interval && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn impostor_scale(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.impostor_scale.filter(|_| {
        a.impostor_inputs && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn color_dither(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.color_dither.filter(|_| {
        a.bark_color_trace && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn bark_dither(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.bark_dither.filter(|_| {
        a.velocity_trace && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_sources_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.group_sources && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_inputs_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.group_inputs && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn terrain_interval_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.terrain_interval && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn base_sources_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.base_sources && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn base_projection_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.base_projection && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_cards_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.group_cards && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_residual_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.group_residual && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn aa_weights_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.aa_weights && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn aa_draws_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.aa_draws && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_owner_four_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.lake_owner_four && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_producer_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.lake_producer && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_gradients_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.lake_gradients && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_waves_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.lake_waves && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_normals_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.lake_normals && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_displacement_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.lake_displacement
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_samplers_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.lake_samplers && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn tess_writer_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.tess_writer && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn tess_owners_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.tess_owners && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn resolve_shadow_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.resolve_shadow && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn dof_inputs_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.dof_inputs && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn fog_generation_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.fog_generation && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn fog_inputs_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.fog_inputs && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn terrain_sources_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.terrain_sources && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn residual_bark_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.residual_bark && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn left_interval_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.left_interval && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn impostor_inputs_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.impostor_inputs && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn interval_trace_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.interval_trace && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn post_geometry_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.post_geometry && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn bark_color_trace_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.bark_color_trace && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn velocity_trace_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.velocity_trace && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn early_depth_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.early_depth
            && a.ssr_operands
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn ssr_operands_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.ssr_operands
            && a.ssr_bracket
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn ssr_bracket_requested() -> bool {
    wants_eyes()
        && STATE.lock().active.as_ref().is_some_and(|a| {
            a.ssr_bracket && a.resolve_samples && matches!(a.phase, Phase::Pending)
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
    if active.late_sky_stages {
        let eyes = if mode == RenderMode::SinglePass {
            [true, false]
        } else {
            required_eyes(selected_eye)
        };
        stages::complete_late_sky(&mut stage_records, frame.unwrap(), eyes);
    }
    if active.water_inputs {
        stages::complete_water_inputs(
            active.water_surface_inputs,
            &mut stage_records,
            frame.unwrap(),
            &active.id.0,
            required_eyes(selected_eye),
        );
    }
    if active.water_reflection.is_some() {
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
        draw_census: active
            .draw_census
            .then(|| draw_census::take(frame.unwrap())),
        resolve_inputs,
        water_reflection: active.water_reflection,
        aa_weights: active.aa_weights.then_some(true),
        aa_draws: active.aa_draws.then_some(true),
        lake_owner_four: active.lake_owner_four.then_some(true),
        lake_producer: active.lake_producer.then_some(true),
        lake_gradients: active.lake_gradients.then_some(true),
        lake_waves: active.lake_waves.then_some(true),
        lake_normals: active.lake_normals.then_some(true),
        lake_displacement: active.lake_displacement.then_some(true),
        aa_color_edge: active.aa_color_edge,
        patch63_hull: active.patch63_hull,
        owner_four_domain: active.owner_four_domain,
        lake_color_mirror: active.lake_color_mirror,
        lake_depth_edge: active.lake_depth_edge,
        lake_ds_mvp: active.lake_ds_mvp,
        lake_inner_edge: active.lake_inner_edge,
        patch_draw_ordinal: active.patch_draw_ordinal,
        second_patch_ordinal: active.second_patch_ordinal,
        cascade_scale: active.cascade_scale,
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
    resolve_samples: bool,
    resolve_eye_camera: Option<bool>,
    proxy_eye: Option<bool>,
    march_eye: Option<bool>,
    veto_eye: Option<bool>,
    grid_eye: Option<bool>,
    history_center: Option<bool>,
    cascade_scale: Option<CascadeScale>,
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

pub(crate) fn frame_identity() -> Option<i64> {
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
                    water_tess_draw: None,
                    patch_draw_ordinal: None,
                    second_patch_ordinal: None,
                    water_surface_inputs: false,
                    late_sky_stages: false,
                    draw_census: false,
                    fog_inputs: false,
                    group_sources: false,
                    group_inputs: false,
                    terrain_interval: false,
                    base_sources: false,
                    base_projection: false,
                    group_cards: false,
                    group_residual: false,
                    aa_weights: false,
                    aa_draws: false,
                    lake_owner_four: false,
                    lake_producer: false,
                    lake_gradients: false,
                    lake_waves: false,
                    lake_normals: false,
                    lake_displacement: false,
                    lake_samplers: false,
                    tess_writer: false,
                    tess_owners: false,
                    resolve_shadow: false,
                    dof_inputs: false,
                    fog_generation: false,
                    sun_inputs: false,
                    resolve_inputs: false,
                    ssr_bracket: false,
                    ssr_operands: false,
                    ssr_prepare: false,
                    ssr_history: false,
                    dof_edge_local: None,
                    card_atlas: None,
                    group_fade: None,
                    group_atlas_scale: None,
                    aa_color_edge: None,
                    patch63_hull: None,
                    owner_four_domain: None,
                    lake_color_mirror: None,
                    lake_depth_edge: None,
                    lake_ds_mvp: None,
                    lake_inner_edge: None,
                    writer_domain_camera: None,
                    base_eye_vp: None,
                    fog_local_uv: None,
                    terrain_camera: None,
                    extra_bark_dither: None,
                    left_atlas_scale: None,
                    impostor_scale: None,
                    color_dither: None,
                    bark_dither: None,
                    terrain_sources: false,
                    residual_bark: false,
                    left_interval: false,
                    impostor_inputs: false,
                    interval_trace: false,
                    post_geometry: false,
                    bark_color_trace: false,
                    velocity_trace: false,
                    early_depth: false,
                    resolve_samples: false,
                    resolve_eye_camera: None,
                    proxy_eye: None,
                    march_eye: None,
                    veto_eye: None,
                    grid_eye: None,
                    history_center: None,
                    cascade_scale: None,
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
            profile: DiagnosticProfile::ControlledV3,
            visual_overrides: None,
            shader_build: None,
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
            exposure: None,
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
