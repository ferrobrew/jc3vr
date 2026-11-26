use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::Ordering,
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

use anyhow::Context as _;
use jc3vr_view::vr::VR_CONFIG;
use parking_lot::Mutex;
pub(crate) mod bound_buffer;
pub(crate) mod constants;
pub(crate) mod draw_census;
mod options;
pub(crate) mod patch_draw;
pub mod register_pipeline;
pub mod register_single_pass;
pub(crate) mod resolve_probe;
pub(crate) mod rewrites;
mod sidecar;
pub mod snapshot;
pub(crate) mod stages;
mod sun_probe;
pub(crate) mod types;
pub(crate) mod water_gpu_probe;

use jc3vr_core::config::engine::Snapshot;
use jc3vr_render::{
    capture_arming::{Arming, arm},
    image_writer,
    image_writer::{ImageTarget, WriteOutcome},
    screenshot::{CaptureSource, read_back},
};
use jc3vr_view::stereo::{dispatch, dispatch::RenderMode};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};

pub use crate::screenshot::harness::options::{
    CaptureRequest, CascadeScale, SunDepthClip, WaterReflection,
};
use crate::{
    screenshot::harness::{
        sidecar::CaptureSidecar,
        types::{
            CameraInput, CaptureKind, CaptureMetadata, CaptureState, CapturedFrame,
            ComparisonInputs, EyeInput, Readiness,
        },
    },
    testing::visual::{HISTORY_FRAMES, VisualMetadata},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(String);

impl RequestId {
    pub fn parse(id: &str) -> Result<Self, &'static str> {
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

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn request(id: RequestId, options: CaptureRequest) {
    if let Err(why) = admissible(&options, false) {
        reject(&id, &why);
        return;
    }
    if options.resolve_eye_camera.is_some() && dispatch::mode() != RenderMode::SinglePass {
        reject(&id, "resolve camera: requires the single-pass mode");
        return;
    }
    if options.cascade_scale.is_some()
        && (dispatch::mode() != RenderMode::Multipass
            || jc3vr_render::stereo::single_pass::shader_identity::status().status != "complete")
    {
        reject(
            &id,
            "cascade treatment: multipass and a complete census required",
        );
        return;
    }
    if let Some(treatment) = options.water_reflection {
        if !treatment.supported(dispatch::mode(), options.water_tess_draw) {
            reject(
                &id,
                "water reflection treatment: incompatible mode or draw selection",
            );
            return;
        }
        let identity = jc3vr_render::stereo::single_pass::shader_identity::status();
        if !identity.requested || identity.status != "complete" {
            reject(
                &id,
                "water reflection treatment: a complete shader identity census is required",
            );
            return;
        }
    }
    request_with_phase(id, None, options);
}

pub fn request_motion(id: RequestId, phase: u32, options: CaptureRequest) {
    if let Err(why) = admissible(&options, true) {
        reject(&id, &why);
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

/// Whether a request's options can run in this payload: they depend on nothing the request leaves
/// out, and a payload built without the capture-only rewrites carries every rewrite they ask for.
/// `motion` says whether the request captures a motion phase.
fn admissible(options: &CaptureRequest, motion: bool) -> Result<(), String> {
    if let Some(why) = options.conflict(motion) {
        return Err(format!("screenshot options: {why}"));
    }
    if !rewrites::AVAILABLE
        && let Some(option) = options.rewriting_option()
    {
        return Err(format!(
            "screenshot options: {option} needs a payload built with the capture-rewrites feature"
        ));
    }
    Ok(())
}

fn request_with_phase(id: RequestId, target_phase: Option<u32>, options: CaptureRequest) {
    arm(Arming::Capture);
    let Some(folder) = jc3vr_core::config::engine::paths::dir().map(|dir| dir.join("captures"))
    else {
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
        active.options = options;
    }
    sync_pending(&state);
}

fn reject(id: &RequestId, error: &str) {
    let Some(folder) = jc3vr_core::config::engine::paths::dir().map(|dir| dir.join("captures"))
    else {
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
        options: CaptureRequest::default(),
        diagnostic_frame: None,
    });
}

/// Whether a screenshot or a snapshot waits for this frame's eyes.
pub(crate) fn wants_eyes() -> bool {
    screenshot_wants_eyes() || snapshot::wants_eyes()
}

/// Whether a screenshot (not a snapshot) waits for this frame's eyes and stages.
pub(crate) fn screenshot_wants_eyes() -> bool {
    match PENDING_STATE.load(Ordering::Acquire) {
        NO_PENDING => false,
        ANY_PHASE => true,
        phase => crate::testing::visual::capture_phase_due(phase),
    }
}

pub(crate) fn diagnostic_capture_id() -> Option<String> {
    if !screenshot_wants_eyes()
        || !crate::testing::visual::ready()
        || !jc3vr_core::hooks::in_gameplay()
        || !jc3vr_view::vr::is_running_published()
        || !jc3vr_view::stereo::active()
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
    active.options.water_reflection.filter(|t| {
        t.supported(dispatch::mode(), active.options.water_tess_draw)
            && matches!(active.phase, Phase::Pending)
    })
}

/// The frame a requested draw census records, while that frame is drawing.
pub(crate) fn draw_census_frame() -> Option<i64> {
    if !screenshot_wants_eyes()
        || !STATE
            .lock()
            .active
            .as_ref()
            .is_some_and(|a| a.options.draw_census)
    {
        return None;
    }
    diagnostic_capture_id()?;
    frame_identity()
}

pub(crate) fn late_sky_stages_requested() -> bool {
    if !screenshot_wants_eyes()
        || !STATE
            .lock()
            .active
            .as_ref()
            .is_some_and(|a| a.options.late_sky_stages)
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
        .is_some_and(|a| a.options.water_surface_inputs && matches!(a.phase, Phase::Pending))
}

pub(crate) fn second_patch_ordinal() -> Option<u8> {
    STATE.lock().active.as_ref()?.options.second_patch_ordinal
}

pub(crate) fn patch_draw_ordinal() -> Option<u8> {
    STATE.lock().active.as_ref()?.options.patch_draw_ordinal
}

pub(crate) fn water_tess_draw() -> Option<u8> {
    STATE.lock().active.as_ref()?.options.water_tess_draw
}

pub(crate) fn water_inputs_requested() -> bool {
    STATE
        .lock()
        .active
        .as_ref()
        .is_some_and(|active| active.options.water_inputs && matches!(active.phase, Phase::Pending))
}

pub(crate) fn sun_inputs_requested() -> bool {
    STATE
        .lock()
        .active
        .as_ref()
        .is_some_and(|active| active.options.sun_inputs && matches!(active.phase, Phase::Pending))
}

pub(crate) fn sun_depth_clip_disabled_right() -> bool {
    if diagnostic_capture_id().is_none() {
        return false;
    }
    STATE.lock().active.as_ref().is_some_and(|active| {
        active.options.sun_depth_clip == Some(SunDepthClip::DisabledRight)
            && matches!(active.phase, Phase::Pending)
    })
}

pub(crate) fn cascade_scale(frame: i64) -> Option<CascadeScale> {
    if dispatch::mode() != RenderMode::Multipass {
        return None;
    }
    let state = STATE.lock();
    let active = state.active.as_ref()?;
    active.options.cascade_scale.filter(|_| {
        active.diagnostic_frame == Some(frame) && matches!(active.phase, Phase::Pending)
    })
}

pub(crate) fn proxy_eye(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.proxy_eye.filter(|_| {
        a.options.ssr_prepare
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn march_eye(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.march_eye.filter(|_| {
        a.options.ssr_prepare
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn veto_eye(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.veto_eye.filter(|_| {
        a.options.ssr_prepare
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn grid_eye(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.grid_eye.filter(|_| {
        a.options.ssr_prepare
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn history_center(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.history_center.filter(|_| {
        a.options.ssr_history
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn resolve_eye_camera(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.resolve_eye_camera.filter(|_| {
        a.options.resolve_inputs
            && a.options.resolve_samples
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn resolve_samples_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.resolve_samples
            && a.options.resolve_inputs
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn ssr_prepare_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.ssr_prepare
            && a.options.ssr_operands
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn ssr_history_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.ssr_history
            && a.options.ssr_operands
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn dof_edge_local(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.dof_edge_local.filter(|_| {
        a.options.dof_inputs
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn card_atlas(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.card_atlas.filter(|_| {
        a.options.group_cards
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_fade(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.group_fade.filter(|_| {
        a.options.group_cards
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_atlas_scale(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.group_atlas_scale.filter(|_| {
        a.options.group_inputs
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn aa_color_edge(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.aa_color_edge.filter(|_| {
        a.options.aa_weights
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn patch63_hull(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.patch63_hull.filter(|_| {
        a.options.lake_owner_four
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn owner_four_domain(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.owner_four_domain.filter(|_| {
        a.options.lake_owner_four
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_color_mirror(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.lake_color_mirror.filter(|_| {
        a.options.lake_producer
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_depth_edge(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.lake_depth_edge.filter(|_| {
        a.options.lake_producer
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_ds_mvp(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.lake_ds_mvp.filter(|_| {
        a.options.lake_producer
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_inner_edge(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.lake_inner_edge.filter(|_| {
        a.options.lake_samplers
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn writer_domain_camera(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.writer_domain_camera.filter(|_| {
        a.options.tess_writer
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn base_eye_vp(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.base_eye_vp.filter(|_| {
        a.options.base_projection
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn fog_local_uv(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.fog_local_uv.filter(|_| {
        a.options.fog_generation
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn terrain_camera(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.terrain_camera.filter(|_| {
        a.options.terrain_sources
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn extra_bark_dither(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.extra_bark_dither.filter(|_| {
        a.options.residual_bark
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn left_atlas_scale(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.left_atlas_scale.filter(|_| {
        a.options.left_interval
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn impostor_scale(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.impostor_scale.filter(|_| {
        a.options.impostor_inputs
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn color_dither(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.color_dither.filter(|_| {
        a.options.bark_color_trace
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn bark_dither(frame: i64) -> Option<bool> {
    let state = STATE.lock();
    let a = state.active.as_ref()?;
    a.options.bark_dither.filter(|_| {
        a.options.velocity_trace
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_sources_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.group_sources
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_inputs_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.group_inputs
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn terrain_interval_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.terrain_interval
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn base_sources_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.base_sources
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn base_projection_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.base_projection
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_cards_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.group_cards
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn group_residual_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.group_residual
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn aa_weights_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.aa_weights
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn aa_draws_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.aa_draws && a.diagnostic_frame == Some(frame) && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_owner_four_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.lake_owner_four
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_producer_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.lake_producer
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_gradients_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.lake_gradients
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_waves_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.lake_waves
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_normals_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.lake_normals
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_displacement_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.lake_displacement
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn lake_samplers_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.lake_samplers
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn tess_writer_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.tess_writer
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn tess_owners_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.tess_owners
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn resolve_shadow_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.resolve_shadow
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn dof_inputs_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.dof_inputs
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn fog_generation_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.fog_generation
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn fog_inputs_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.fog_inputs
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn terrain_sources_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.terrain_sources
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn residual_bark_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.residual_bark
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn left_interval_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.left_interval
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn impostor_inputs_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.impostor_inputs
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn interval_trace_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.interval_trace
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn post_geometry_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.post_geometry
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn bark_color_trace_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.bark_color_trace
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn velocity_trace_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.velocity_trace
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn early_depth_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.early_depth
            && a.options.ssr_operands
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn ssr_operands_requested(frame: i64) -> bool {
    STATE.lock().active.as_ref().is_some_and(|a| {
        a.options.ssr_operands
            && a.options.ssr_bracket
            && a.diagnostic_frame == Some(frame)
            && matches!(a.phase, Phase::Pending)
    })
}

pub(crate) fn ssr_bracket_requested() -> bool {
    screenshot_wants_eyes()
        && STATE.lock().active.as_ref().is_some_and(|a| {
            a.options.ssr_bracket && a.options.resolve_samples && matches!(a.phase, Phase::Pending)
        })
}

pub(crate) fn resolve_inputs_requested() -> bool {
    STATE.lock().active.as_ref().is_some_and(|active| {
        active.options.resolve_inputs && matches!(active.phase, Phase::Pending)
    })
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

/// Whether the eyes `selected` names (both when `None`) were copied fresh for `frame`.
pub(crate) fn eyes_fresh(frame: i64, selected: Option<usize>) -> bool {
    let fresh = STATE.lock().eyes.map(|eye| eye == Some(frame));
    required_eyes_fresh(selected, fresh)
}

pub(crate) fn capture(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: CaptureSource<'_>,
) {
    if !screenshot_wants_eyes() {
        return;
    }
    let frame = frame_identity();
    let running = jc3vr_view::vr::is_running_published();
    let gameplay = jc3vr_core::hooks::in_gameplay();
    let effective_config = Snapshot::take();
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
        jc3vr_view::stereo::active(),
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
    if active.options.late_sky_stages {
        let eyes = if mode == RenderMode::SinglePass {
            [true, false]
        } else {
            required_eyes(selected_eye)
        };
        stages::complete_late_sky(&mut stage_records, frame.unwrap(), eyes);
    }
    if active.options.water_inputs {
        stages::complete_water_inputs(
            active.options.water_surface_inputs,
            &mut stage_records,
            frame.unwrap(),
            &active.id.0,
            required_eyes(selected_eye),
        );
    }
    if active.options.water_reflection.is_some() {
        stages::complete_water_treatment(
            &mut stage_records,
            frame.unwrap(),
            &active.id.0,
            required_eyes(selected_eye),
        );
    }
    if active.options.sun_inputs {
        stages::complete_sun_inputs(
            &mut stage_records,
            frame.unwrap(),
            &active.id.0,
            required_eyes(selected_eye),
        );
    }
    if active.options.sun_depth_clip.is_some() && mode == RenderMode::SinglePass {
        stages::complete_sun_treatment(&mut stage_records, frame.unwrap(), &active.id.0);
    }
    let resolve_inputs = active.options.resolve_inputs.then(|| {
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
            .options
            .draw_census
            .then(|| draw_census::take(frame.unwrap())),
        resolve_inputs,
        water_reflection: active.options.water_reflection,
        aa_weights: active.options.aa_weights.then_some(true),
        aa_draws: active.options.aa_draws.then_some(true),
        lake_owner_four: active.options.lake_owner_four.then_some(true),
        lake_producer: active.options.lake_producer.then_some(true),
        lake_gradients: active.options.lake_gradients.then_some(true),
        lake_waves: active.options.lake_waves.then_some(true),
        lake_normals: active.options.lake_normals.then_some(true),
        lake_displacement: active.options.lake_displacement.then_some(true),
        aa_color_edge: active.options.aa_color_edge,
        patch63_hull: active.options.patch63_hull,
        owner_four_domain: active.options.owner_four_domain,
        lake_color_mirror: active.options.lake_color_mirror,
        lake_depth_edge: active.options.lake_depth_edge,
        lake_ds_mvp: active.options.lake_ds_mvp,
        lake_inner_edge: active.options.lake_inner_edge,
        patch_draw_ordinal: active.options.patch_draw_ordinal,
        second_patch_ordinal: active.options.second_patch_ordinal,
        cascade_scale: active.options.cascade_scale,
        sun_depth_clip: active.options.sun_depth_clip,
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
        ImageTarget::Attached {
            path: png,
            sidecar: Box::new(CaptureSidecar::new(Box::new(sidecar), stage_images)),
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

pub fn tick() {
    snapshot::tick();
    let latest_readiness = screenshot_wants_eyes().then(readiness);
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
                let written = CaptureSidecar::take_written(outcome.sidecar);
                if let Some(capture) = &mut active.result.capture
                    && !written.stages.is_empty()
                {
                    capture.stages = written.stages;
                }
                match outcome.result {
                    Ok(_) => {
                        if let Some(capture) = &mut active.result.capture {
                            capture.resolve_inputs = written.resolve_inputs;
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
    options: CaptureRequest,
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
    jc3vr_view::vr::capture_views().map(|views| views.predicted_display_time_ns)
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

pub(crate) fn motion_capture_gate(
    visual: &VisualMetadata,
    target: u32,
    frame: i64,
) -> Result<(), String> {
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
        jc3vr_view::vr::pose_control::current().map(|pose| CameraInput {
            position: pose.position.to_array(),
            orientation: pose.orientation().to_array(),
        })
    };
    let eyes = [0, 1].map(|eye| {
        jc3vr_view::vr::render_params(eye).map(|params| EyeInput {
            offset: params.local_offset.to_array(),
            orientation_delta: params.orientation_delta.to_array(),
            projection_standard: glam::Mat4::from(params.projection_standard).to_cols_array(),
            projection_reverse_z: glam::Mat4::from(params.projection_reverse_z).to_cols_array(),
        })
    });
    ComparisonInputs {
        build_stamp: jc3vr_core::lifecycle::build_stamp(),
        profile: visual.profile,
        camera,
        eyes,
        eye_width,
        eye_height,
        world_scale: VR_CONFIG.query(|config| config.world_scale),
    }
}

fn readiness() -> Readiness {
    Readiness {
        gameplay_ready: jc3vr_core::hooks::in_gameplay(),
        xr_session_running: jc3vr_view::vr::is_running_published(),
        xr_frame_rendering: frame_identity().is_some(),
        build_stamp: jc3vr_core::lifecycle::build_stamp(),
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
                    options: CaptureRequest::default(),
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
            shader_identity: jc3vr_render::stereo::single_pass::shader_identity::status(),
            ready: true,
            paused: true,
            frozen: true,
            error: None,
            warmup_frames: 1,
            completed_frames: 1,
            configure_frames: 1,
            engine_dimensions: Some((4096, 2048)),
            pose: None,
            environment: None,
            exposure: None,
            headset: None,
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
