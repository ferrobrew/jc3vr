//! FSR anti-aliasing / upscaling integration.
//!
//! Runs FSR2 (via [`fsr_sys`]) in place of the engine's SMAA: per eye, it resolves the post-tonemap
//! scene color (plus the engine's depth and velocity) into our own output texture, which is then
//! copied back into the post-effect chain's working slot. The engine AA is suppressed while FSR is
//! active. See `docs/mod/rendering/fsr.md` for the dispatch-point rationale and the AA-first/upscaler-later plan.
//!
//! State is one [`fsr_sys::Context`] and one output texture per eye, lazily (re)created whenever the
//! render resolution changes -- the same compare-and-recreate pattern the debug captures use, which
//! self-heals after the engine resizes (`Graphics::Reset` -> `CreateRenderSetups`) rather than racing
//! it. All of this lives on the render thread, behind [`FSR_STATE`]; dispatch runs under the engine's
//! context mutex like the other D3D11 work.
//!
//! Under the single-pass collapse the chain's textures hold both eyes side by side. With
//! `single_pass.fsr_per_eye` on, each eye's half is copied into eye-sized inputs and resolved through
//! that eye's own context, then copied back ([`EyeLayout::SideBySide`]), so each eye's history and
//! reconstruction see only that eye, at its own size, as an eye rendered on its own does.
//!
//! Each VR frame's projections carry a sub-pixel jitter ([`jitter`]), which FSR is told per eye in
//! its own pixels; the motion vectors it is handed are free of it ([`jitter::jitter_free_reprojection`]).

pub mod taa;
mod velocity_decode;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fsr_sys::{Context, DispatchInfo, Extent, init_flags};
use jc3gi::{
    graphics_engine::{device::Device, graphics_engine::GraphicsEngine, texture::Texture},
    types::math::Matrix4,
};
use jc3vr_view::{
    fsr::{FSR_CONFIG, jitter},
    hooks::camera::SCENE_NEAR_CLIP_M,
    stereo::{collapse_eye, with_vp_history},
};
use parking_lot::Mutex;
use serde::Serialize;
use velocity_decode::VelocityDecode;
use windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS, D3D11_BOX,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11DeviceContext, ID3D11Resource,
            ID3D11Texture2D,
        },
        Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC},
    },
    core::Interface,
};

use crate::{probes, probes::PipelinePoint, tuning::RENDER};

/// How the post chain's textures hold the eyes one FSR resolve covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EyeLayout {
    /// The textures hold one eye, the dispatch's: resolved whole through that eye's context.
    Single(usize),
    /// The textures hold both eyes side by side, eye 0 in the left half (the single-pass collapse's
    /// double-wide target): each half is resolved through its own eye's context at one eye's size.
    SideBySide,
}

/// The engine textures one resolve reads and writes.
pub struct ChainTextures<'a> {
    /// The chain's current result texture: the resolve's colour input and its copy-back target.
    pub(crate) slot_color: &'a Texture,
    /// The engine's main depth buffer.
    pub depth: &'a Texture,
    /// The engine's velocity buffer.
    pub velocity: &'a Texture,
}

/// Per-eye FSR state, recreated together when the render size changes.
struct EyeState {
    context: Context,
    /// The UAV output target FSR writes into (display-res; copied back into the post chain).
    output: ID3D11Texture2D,
    /// The eye-sized copy of the eye's half of the chain colour, for a side-by-side resolve.
    input: Option<ID3D11Texture2D>,
    /// The velocity-decode pass + its R16G16F MV buffer (None if it failed to build -- FSR then runs
    /// with no motion vectors).
    decode: Option<VelocityDecode>,
    /// Set on creation; the first dispatch consumes it as FSR's `reset` so a fresh context (after a
    /// resize / toggle-on) discards any garbage history instead of reprojecting against it.
    fresh: bool,
}

/// The size and layout the eye contexts were built for.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Build {
    eye_width: u32,
    height: u32,
    side_by_side: bool,
}

impl Build {
    /// The eye size and layout a resolve of a `width` x `height` chain texture holding `layout`
    /// needs.
    fn for_layout(layout: EyeLayout, width: u32, height: u32) -> Self {
        let side_by_side = layout == EyeLayout::SideBySide;
        Self {
            eye_width: if side_by_side { width / 2 } else { width },
            height,
            side_by_side,
        }
    }
}

/// The live FSR integration state (render thread only).
pub struct FsrState {
    eyes: [Option<EyeState>; 2],
    /// What the current eye contexts/outputs were built for; recreate on change.
    build: Option<Build>,
    /// Each eye's view-projections from the last side-by-side resolve, for the stereo correction.
    side_by_side_history: [Option<EyeViewProjections>; 2],
}
impl FsrState {
    const fn new() -> Self {
        Self {
            eyes: [None, None],
            build: None,
            side_by_side_history: [None, None],
        }
    }
}

/// One eye's view-projections for a frame, as an eye-sized render of that eye holds them.
#[derive(Clone, Copy)]
struct EyeViewProjections {
    /// The centre camera's view with the eye's projection: what the engine's velocity pass takes
    /// the previous frame through when it renders the eye on its own.
    center: glam::Mat4,
    /// The eye's own view-projection, without the FSR jitter.
    eye: glam::Mat4,
    /// The FSR jitter's clip-space offset the eye rasterizes with, on top of [`eye`](Self::eye).
    jitter: glam::Vec2,
}

impl EyeViewProjections {
    /// This frame's matrices for `eye` under the single-pass collapse, where the render camera stays
    /// centred and each eye's view-projection comes from the VR frame's per-eye parameters.
    fn current(eye: usize) -> Option<Self> {
        let eye = collapse_eye(eye)?;
        let projection = eye.params.unjittered_projection_reverse_z();
        Some(Self {
            center: projection * eye.center.inverse(),
            eye: projection * eye.world().inverse(),
            jitter: eye.params.jitter_ndc(),
        })
    }
}

/// One eye's motion inputs for a temporal resolve this frame: what the velocity decode reprojects
/// with, and what the harness records of it. Shared by FSR and the mod's own resolve ([`taa`]).
struct EyeMotion {
    /// The clip-to-previous-clip matrices (the encoded camera, the eye), when a frame of history
    /// exists for them.
    reprojection: Option<(Matrix4, Matrix4)>,
    views: Option<RecordedViews>,
    /// The jitter the eye's image carries this frame, as a clip-space offset on top of the
    /// recorded view-projections, which carry none.
    jitter_ndc: glam::Vec2,
    /// The previous view-projection the engine's velocity pass encoded against, when known.
    encoded_previous: Option<glam::Mat4>,
}

impl EyeMotion {
    /// The eye's motion inputs this frame. A side-by-side resolve takes the reprojection from each
    /// eye's own matrices, which the collapse's centred render camera does not carry, rotating
    /// `side_by_side_history` (the resolver's own record of the eye's last frame); a whole resolve
    /// takes it from the render camera's history.
    fn this_frame(
        eye: usize,
        side_by_side: bool,
        side_by_side_history: &mut Option<EyeViewProjections>,
    ) -> Self {
        let (reprojection, views, jitter_ndc) = if side_by_side {
            let current = EyeViewProjections::current(eye);
            let previous = std::mem::replace(side_by_side_history, current);
            let pair = previous.zip(current);
            (
                pair.map(|(previous, current)| side_by_side_reprojection(previous, current)),
                pair.map(|(previous, current)| RecordedViews {
                    current_eye: current.eye.to_cols_array(),
                    previous_eye: previous.eye.to_cols_array(),
                    current_center: current.center.to_cols_array(),
                    previous_center: previous.center.to_cols_array(),
                }),
                current.map_or(glam::Vec2::ZERO, |current| current.jitter),
            )
        } else {
            with_vp_history(|history| {
                (
                    history.reprojection_matrices(eye),
                    RecordedViews::of_history(history, eye),
                    history.cur_jitter[eye],
                )
            })
        };
        // The engine's velocity pass encodes the motion against its render camera's own previous
        // view-projection (taken through the single-pass rewrite's map for an eye of the collapse),
        // which need not be the centre camera the mod snapshots: when it is known, the stereo
        // correction re-anchors against it.
        let encoded_previous = encoded_previous_view_projection(eye, side_by_side);
        let reprojection = match (reprojection, encoded_previous, views) {
            (Some((_, eye_reprojection)), Some(previous), Some(views)) => {
                let current = glam::Mat4::from_cols_array(&views.current_eye);
                Some((
                    jitter::reprojection_from_jittered(previous, current, jitter_ndc),
                    eye_reprojection,
                ))
            }
            (reprojection, ..) => reprojection,
        };
        Self {
            reprojection,
            views,
            jitter_ndc,
            encoded_previous,
        }
    }
}

/// The stereo correction's clip-to-previous-clip matrices (centre, eye) from `previous` to
/// `current`, formed in `f64` as [`jc3vr_view::stereo::VpHistory::reprojection_matrices`] forms them for
/// an eye rendered on its own.
fn side_by_side_reprojection(
    previous: EyeViewProjections,
    current: EyeViewProjections,
) -> (Matrix4, Matrix4) {
    (
        jitter::reprojection_from_jittered(previous.center, current.eye, current.jitter),
        jitter::jitter_free_reprojection(previous.eye, current.eye, current.jitter),
    )
}

/// The previous view-projection the engine's velocity pass encodes `eye`'s motion against: the
/// render camera's previous view-projection, and for an eye of the single-pass collapse that taken
/// through the velocity rewrite's map into the eye's own clip space. `None` while the engine or the
/// map is unavailable.
fn encoded_previous_view_projection(eye: usize, side_by_side: bool) -> Option<glam::Mat4> {
    // SAFETY: the render thread at the anti-aliasing hook, where the engine's render camera is live
    // and not being written.
    let camera = unsafe { &GraphicsEngine::get()?.m_RenderCamera };
    let previous = glam::Mat4::from(camera.m_PreviousViewProjectionF);
    if !side_by_side {
        return Some(previous);
    }
    let maps = crate::stereo::single_pass::eye_camera::velocity_maps()?;
    Some(maps[eye] * previous)
}

/// Whether the eye's clip-to-previous-clip reprojection is a camera cut rather than motion: the
/// view centre, at the infinite far plane (the rotation) or at the tuning's near probe depth (the
/// rotation and the translation), lands further than the tuning's share of the half-screen away in
/// the previous frame ([`crate::tuning::CameraCutTuning`]). No head moves that far in one frame; a
/// teleport or a camera switch does.
fn camera_cut(eye_reprojection: &Matrix4) -> bool {
    let tuning = &RENDER.get().camera_cut;
    let matrix = glam::Mat4::from(*eye_reprojection);
    // Reverse-Z with an infinite far plane: the depth is the near distance over the view depth.
    [0.0, SCENE_NEAR_CLIP_M / tuning.probe_depth_m]
        .into_iter()
        .any(|depth| {
            let previous = matrix * glam::Vec4::new(0.0, 0.0, depth, 1.0);
            let ndc = previous.truncate().truncate() / previous.w;
            previous.w.is_nan()
                || previous.w <= 0.0
                || !ndc.is_finite()
                || ndc.length() > tuning.ndc
        })
}

/// Global FSR state. Locked briefly on the render thread at the AA hook.
pub(crate) static FSR_STATE: Mutex<FsrState> = Mutex::new(FsrState::new());

/// Release both eyes' contexts, outputs, and velocity decodes, and the temporal resolve's
/// textures, for eject. The state lives in a
/// static, which an unloading payload never drops, so without this every resource FSR built stays
/// allocated on the game's device after the payload is gone.
pub fn release_on_eject() {
    *FSR_STATE.lock() = FsrState::new();
    taa::release_on_eject();
}

/// Discard every eye's history at the next resolve, so the accumulation restarts from that frame,
/// and restart the jitter sequence. The visual harness calls it on every configuration, so each mode
/// it compares accumulates from the same number of frames, through the same jitter offsets, rather
/// than inheriting another mode's history.
pub fn request_history_reset() {
    RESET_REQUESTED.store(true, Ordering::Relaxed);
    taa::request_history_reset();
    jitter::reset_phase();
}

/// How many eye halves the last resolve covered side by side (0 when it resolved whole).
pub fn split_dispatch_count() -> usize {
    SPLIT_DISPATCHES.load(Ordering::Relaxed)
}

/// Drive an FSR resolve over the current frame's buffers, writing the anti-aliased result back into
/// the chain's working LDR texture. Returns `true` if FSR ran for any eye; `false` means it fell
/// through and the engine AA should proceed as normal.
///
/// `device` is the engine D3D11 device and `sharpness` the optional RCAS strength. Runs on the render
/// thread; holds the engine context mutex across the velocity decode, the FSR dispatch, and the
/// copy-back of each eye.
pub fn dispatch(
    state: &mut FsrState,
    device: &Device,
    textures: &ChainTextures<'_>,
    layout: EyeLayout,
    sharpness: Option<f32>,
) -> bool {
    // Nothing is built once eject has begun: the eject's release would not see it.
    if jc3vr_core::is_shutting_down() {
        return false;
    }
    let width = u32::from(textures.slot_color.m_Width);
    let height = u32::from(textures.slot_color.m_Height);
    let build = Build::for_layout(layout, width, height);
    if build.eye_width == 0 || height == 0 {
        return false;
    }

    // (Re)build both eyes' contexts + outputs if the render size or the layout changed.
    if state.build != Some(build) {
        state.eyes = [None, None];
        state.build = Some(build);
        state.side_by_side_history = [None, None];
    }
    if RESET_REQUESTED.swap(false, Ordering::Relaxed) {
        for eye in state.eyes.iter_mut().flatten() {
            eye.fresh = true;
        }
    }

    // SAFETY: the engine's context wrapper is live for the duration of this render-thread call.
    let Some(context) = (unsafe { device.m_Context.as_ref() }) else {
        return false;
    };
    let camera = camera_params();
    jitter::publish_eye_size(build.eye_width, build.height);
    let (eyes, regions): (&[usize], [Option<u32>; 2]) = match layout {
        EyeLayout::Single(eye) => {
            let eye = eye.min(1);
            (&EYES[eye..=eye], [None, None])
        }
        EyeLayout::SideBySide => (&EYES, [Some(0), Some(build.eye_width)]),
    };
    let stereo = jc3vr_view::stereo::active();
    let (correction, hybrid) = FSR_CONFIG.query(|fsr| {
        (
            fsr.stereo_mv_correction && stereo,
            fsr.hybrid_velocity && stereo,
        )
    });
    let frame = jc3vr_view::vr::capture_views().map(|views| views.predicted_display_time_ns);
    let mut ran = false;
    for &eye in eyes {
        let EyeMotion {
            reprojection,
            views,
            jitter_ndc,
            encoded_previous,
        } = EyeMotion::this_frame(
            eye,
            build.side_by_side,
            &mut state.side_by_side_history[eye],
        );
        let reprojection = reprojection.filter(|_| correction || hybrid);
        if state.eyes[eye].is_none() {
            state.eyes[eye] = create_eye(device, textures.slot_color, build);
        }
        let Some(eye_state) = state.eyes[eye].as_mut() else {
            continue;
        };
        // A camera cut (a teleport, a camera switch, a configuration jumping the pose) leaves no
        // history worth reprojecting: discard it, and send no motion against the old frame.
        let cut = reprojection.is_some_and(|(_, eye_reprojection)| camera_cut(&eye_reprojection));
        if cut {
            eye_state.fresh = true;
        }
        let reset = eye_state.fresh;
        // The same offset in FSR's pixels.
        let jitter_pixels = jitter::pixels_from_ndc(jitter_ndc, build.eye_width, build.height);
        let request = EyeResolve {
            eye,
            jitter: jitter_pixels,
            region_x: regions[eye],
            reprojection: reprojection.filter(|_| !cut),
            correct_parallax: correction,
            hybrid,
            build,
            sharpness,
            camera: &camera,
        };
        // SAFETY: `context.m_Mutex` guards the immediate context; every resource is valid for the
        // calls, all of which `resolve_eye` records under the mutex.
        let resolved = unsafe {
            windows::Win32::System::Threading::EnterCriticalSection(context.m_Mutex);
            let resolved = resolve_eye(eye_state, &context.m_Context, device, textures, &request);
            windows::Win32::System::Threading::LeaveCriticalSection(context.m_Mutex);
            resolved
        };
        ran |= resolved;
        RECORDS.lock()[eye] = Some(DispatchRecord {
            frame,
            eye,
            side_by_side: build.side_by_side,
            render_size: [build.eye_width, build.height],
            resolved,
            reset,
            camera_cut: cut,
            correct_parallax: correction,
            hybrid,
            sign: [MV_SIGN.0, MV_SIGN.1],
            motion_vector_scale: [build.eye_width as f32, build.height as f32],
            near: camera.near,
            far: camera.far,
            fov_vertical: camera.fov_vertical,
            views,
            reprojection: request
                .reprojection
                .map(|(center, eye)| RecordedReprojection {
                    center: center.data,
                    eye: eye.data,
                }),
            encoded_previous: encoded_previous.map(|matrix| matrix.to_cols_array()),
            jitter_pixels: jitter_pixels.to_array(),
            jitter_ndc: jitter_ndc.to_array(),
            jitter_phase: jc3vr_view::vr::render_params(eye)
                .and_then(|params| params.jitter)
                .map(|jitter| jitter.phase),
        });
    }
    SPLIT_DISPATCHES.store(
        if build.side_by_side { eyes.len() } else { 0 },
        Ordering::Relaxed,
    );
    ran
}

/// What FSR resolved each eye with on `frame`, for the capture harness's snapshots: the eyes whose
/// last resolve was on that frame.
pub fn dispatch_records(frame: i64) -> Vec<DispatchRecord> {
    RECORDS
        .lock()
        .iter()
        .flatten()
        .filter(|record| record.frame == Some(frame))
        .cloned()
        .collect()
}

/// One eye's FSR resolve, as the capture harness records it: the inputs that decide the motion
/// vectors FSR is handed, for checking them offline against the depth.
#[derive(Clone, Serialize)]
pub struct DispatchRecord {
    /// The harness's frame identity of the frame the resolve ran on.
    frame: Option<i64>,
    eye: usize,
    /// Whether the eye was resolved as one half of the single-pass double-wide target.
    side_by_side: bool,
    /// The eye's render size, in pixels.
    render_size: [u32; 2],
    /// Whether FSR ran.
    resolved: bool,
    /// Whether the resolve discarded the eye's history.
    reset: bool,
    /// Whether the reprojection was taken for a camera cut, and so withheld from the decode.
    camera_cut: bool,
    correct_parallax: bool,
    hybrid: bool,
    /// The decode's sign convention, applied to its UV motion ([`MV_SIGN`]).
    sign: [f32; 2],
    /// FSR's motion-vector scale, which maps the decode's UV motion to pixels.
    motion_vector_scale: [f32; 2],
    near: f32,
    far: f32,
    fov_vertical: f32,
    /// The eye's view-projections, this frame's and last frame's, when both exist.
    views: Option<RecordedViews>,
    /// The clip-to-previous-clip matrices the decode reprojected with, when it reprojected.
    reprojection: Option<RecordedReprojection>,
    /// The previous view-projection the engine's velocity pass encoded the eye's motion against,
    /// column-major, when it is known.
    encoded_previous: Option<[f32; 16]>,
    /// The jitter FSR was told, in the eye's pixels (x right, y down).
    jitter_pixels: [f32; 2],
    /// The same jitter as the clip-space offset the eye rasterized with on top of the recorded
    /// view-projections, which carry none.
    jitter_ndc: [f32; 2],
    /// The jitter's index in its sequence, when the jitter runs.
    jitter_phase: Option<u32>,
}

/// One eye's view-projections, column-major, without the FSR jitter: its own and the centre camera's
/// (with the eye's projection), this frame's and last frame's.
#[derive(Clone, Copy, Serialize)]
pub(crate) struct RecordedViews {
    current_eye: [f32; 16],
    previous_eye: [f32; 16],
    current_center: [f32; 16],
    previous_center: [f32; 16],
}

impl RecordedViews {
    /// The render camera's history for `eye`, when it holds both frames.
    fn of_history(history: &jc3vr_view::stereo::VpHistory, eye: usize) -> Option<Self> {
        Some(Self {
            current_eye: history.cur_eye.get(eye).copied()??.to_cols_array(),
            previous_eye: history.prev_eye.get(eye).copied()??.to_cols_array(),
            current_center: history.cur_center.get(eye).copied()??.to_cols_array(),
            previous_center: history.prev_center.get(eye).copied()??.to_cols_array(),
        })
    }
}

/// The decode's clip-to-previous-clip matrices, column-major as uploaded.
#[derive(Clone, Copy, Serialize)]
pub(crate) struct RecordedReprojection {
    center: [f32; 16],
    eye: [f32; 16],
}

static RECORDS: Mutex<[Option<DispatchRecord>; 2]> = Mutex::new([None, None]);
static RESET_REQUESTED: AtomicBool = AtomicBool::new(false);
static SPLIT_DISPATCHES: AtomicUsize = AtomicUsize::new(0);
static EYES: [usize; 2] = [0, 1];

/// One eye's resolve within a [`dispatch`].
struct EyeResolve<'a> {
    eye: usize,
    /// The jitter this eye's projections carry this frame, in its pixels (FSR's `jitterOffset`).
    jitter: glam::Vec2,
    /// The stereo correction's matrices (centre, eye), when it applies; see
    /// [`velocity_decode::DecodeInputs::reprojection`].
    reprojection: Option<(Matrix4, Matrix4)>,
    /// See [`velocity_decode::DecodeInputs::correct_parallax`].
    correct_parallax: bool,
    /// See [`velocity_decode::DecodeInputs::hybrid`].
    hybrid: bool,
    /// The first column of the eye's half when the textures hold both eyes; `None` when they hold
    /// only this eye.
    region_x: Option<u32>,
    build: Build,
    sharpness: Option<f32>,
    camera: &'a CameraParams,
}

/// Resolve one eye: copy its half in when the textures hold both eyes, decode its motion vectors,
/// dispatch FSR, and copy the result back into the chain's working slot.
///
/// # Safety
/// `context` must be the engine's immediate context, held under its mutex.
unsafe fn resolve_eye(
    eye_state: &mut EyeState,
    context: &ID3D11DeviceContext,
    device: &Device,
    textures: &ChainTextures<'_>,
    request: &EyeResolve<'_>,
) -> bool {
    let &EyeResolve {
        eye,
        jitter,
        reprojection,
        correct_parallax,
        hybrid,
        region_x,
        build,
        sharpness,
        camera,
    } = request;
    let output_res: ID3D11Resource = eye_state.output.cast().expect("texture is a resource");
    let region = region_x.map(|left| D3D11_BOX {
        left,
        top: 0,
        front: 0,
        right: left + build.eye_width,
        bottom: build.height,
        back: 1,
    });

    // SAFETY: the caller holds the context mutex; every resource is live for these calls.
    unsafe {
        let color: ID3D11Resource = match (region, eye_state.input.as_ref()) {
            (None, _) => textures.slot_color.m_Texture.clone(),
            (Some(region), Some(input)) => {
                context.CopySubresourceRegion(
                    input,
                    0,
                    0,
                    0,
                    0,
                    &textures.slot_color.m_Texture,
                    0,
                    Some(&region),
                );
                input.cast().expect("texture is a resource")
            }
            (Some(_), None) => return false,
        };

        // Decode JC3's bias-encoded velocity into FSR's MV buffer (re-anchoring the vectors at this
        // eye's own previous pose when the stereo correction is active); on success FSR reads the
        // decoded buffer, otherwise it falls back to the raw velocity (still biased -- only used if
        // the decode is unavailable or disabled, as a degraded path). A side-by-side resolve needs
        // the decode's depth copy, so it cannot run without it.
        let decoded_mv = eye_state.decode.as_ref().filter(|d| {
            d.dispatch(
                context,
                device,
                &velocity_decode::DecodeInputs {
                    velocity: textures.velocity,
                    depth: textures.depth,
                    sign: MV_SIGN,
                    reprojection,
                    correct_parallax,
                    hybrid,
                    offset: (region_x.unwrap_or(0), 0),
                },
            )
        });
        let (mv_res, depth_res): (ID3D11Resource, ID3D11Resource) = match (region, decoded_mv) {
            (None, Some(d)) => (
                d.output.cast().expect("texture is a resource"),
                textures.depth.m_Texture.clone(),
            ),
            (None, None) => (
                textures.velocity.m_Texture.clone(),
                textures.depth.m_Texture.clone(),
            ),
            (Some(_), Some(d)) => match d.depth_copy.as_ref() {
                Some(copy) => (
                    d.output.cast().expect("texture is a resource"),
                    copy.texture.cast().expect("texture is a resource"),
                ),
                None => return false,
            },
            (Some(_), None) => return false,
        };
        probes::point(PipelinePoint::FsrInputs {
            eye,
            motion: &mv_res,
            depth: &depth_res,
        });

        let info = DispatchInfo {
            context,
            color: &color,
            depth: &depth_res,
            motion_vectors: &mv_res,
            exposure: None,
            output: &output_res,
            render_size: Extent {
                width: build.eye_width,
                height: build.height,
            },
            jitter: (jitter.x, jitter.y),
            // The decode emits motion in UV space; FSR wants pixels, so scale by the render size. The
            // sign/Y convention is already applied inside the decode shader.
            motion_vector_scale: if decoded_mv.is_some() {
                (build.eye_width as f32, build.height as f32)
            } else {
                (1.0, 1.0)
            },
            sharpening: sharpness,
            frame_time_delta_ms: camera.frame_time_delta_ms,
            pre_exposure: 1.0,
            // Discard history on the first dispatch of a freshly created context.
            reset: std::mem::take(&mut eye_state.fresh),
            camera_near: camera.near,
            camera_far: camera.far,
            camera_fov_vertical: camera.fov_vertical,
        };

        let ran = eye_state.context.dispatch(&info);
        if ran {
            // Copy the AA'd result back into the chain's working slot so the rest of the post chain
            // (and the back-buffer capture) sees it.
            match region_x {
                None => context.CopyResource(&textures.slot_color.m_Texture, &output_res),
                Some(left) => context.CopySubresourceRegion(
                    &textures.slot_color.m_Texture,
                    0,
                    left,
                    0,
                    0,
                    &output_res,
                    0,
                    None,
                ),
            }
        }
        ran
    }
}

/// The sign convention applied to the decoded UV motion before FSR. The decode yields the engine's
/// convention, the current UV less the previous one with Y down (see `docs/mod/rendering/fsr.md`);
/// FSR 2 reads a pixel's history at its current UV plus its motion vector, in the same Y-down UV
/// (`ffx_fsr2_reproject.h`), so it wants the previous UV less the current one: both axes negate.
const MV_SIGN: (f32, f32) = (-1.0, -1.0);

/// Build one eye's FSR context + UAV output texture at the build's eye size, matching the chain
/// color's format, with an eye-sized colour input and depth copy when the textures hold both eyes.
/// Returns `None` on any failure (the caller then falls back to engine AA for this eye).
fn create_eye(device: &Device, slot_color: &Texture, build: Build) -> Option<EyeState> {
    let d3d = &device.m_Device;
    let Build {
        eye_width: width,
        height,
        side_by_side,
    } = build;
    let texture = |bind_flags: u32, label: &str| {
        let mut texture: Option<ID3D11Texture2D> = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT(slot_color.m_Format as i32),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bind_flags,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        // SAFETY: valid device + desc; `texture` receives the created texture.
        if unsafe { d3d.CreateTexture2D(&desc, None, Some(&mut texture)) }.is_err() {
            tracing::error!("FSR: failed to create the {label} texture");
        }
        texture
    };
    // The output is sampled by the copy-back and written by FSR, so it needs both bind flags.
    let output = texture(
        (D3D11_BIND_UNORDERED_ACCESS.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        "output",
    )?;
    let input = if side_by_side {
        Some(texture(D3D11_BIND_SHADER_RESOURCE.0 as u32, "eye input")?)
    } else {
        None
    };

    // Native AA: render size == display size. The HDR / depth / exposure flags match how we feed FSR
    // (post-tonemap LDR for now, so no HDR flag; reverse-Z infinite-far depth; FSR auto-exposure).
    let flags = init_flags::DEPTH_INVERTED | init_flags::DEPTH_INFINITE | init_flags::AUTO_EXPOSURE;
    let extent = Extent { width, height };
    let context = Context::new(d3d, flags, extent, extent)?;
    let decode = VelocityDecode::new(device, width, height, side_by_side);
    if decode.is_none() {
        tracing::warn!("FSR: velocity-decode pass unavailable; running without motion vectors");
    }
    Some(EyeState {
        context,
        output,
        input,
        decode,
        fresh: true,
    })
}

/// The render camera's parameters FSR's depth reprojection needs this frame.
struct CameraParams {
    near: f32,
    far: f32,
    /// Vertical field of view in radians.
    fov_vertical: f32,
    frame_time_delta_ms: f32,
}

/// Fallbacks when the camera / clock singletons aren't reachable yet.
const FALLBACK_FAR: f32 = 10000.0;
const FALLBACK_FOV_VERTICAL: f32 = std::f32::consts::FRAC_PI_2;
const FALLBACK_FRAME_MS: f32 = 1000.0 / 60.0;

/// Read the live render camera's near/far/FOV and the real frame time. The vertical FOV is derived
/// from the projection's `data[5]` (`= 1/tan(fovV/2)`) rather than `m_FOV`, which sidesteps the
/// horizontal-vs-vertical question and is invariant under the reverse-Z we apply.
fn camera_params() -> CameraParams {
    // SAFETY: the camera-manager and clock singletons are valid once the engine is initialised; both
    // accessors null-check the underlying pointer.
    unsafe {
        let mut params = CameraParams {
            near: SCENE_NEAR_CLIP_M,
            far: FALLBACK_FAR,
            fov_vertical: FALLBACK_FOV_VERTICAL,
            frame_time_delta_ms: FALLBACK_FRAME_MS,
        };
        if let Some(cm) = jc3gi::camera::camera_manager::CameraManager::get()
            && let Some(camera) = cm.GetRenderCamera().as_ref()
        {
            params.near = camera.m_Near;
            params.far = camera.m_Far;
            let focal_y = camera.m_Projection.data[5];
            if focal_y.abs() > f32::EPSILON {
                params.fov_vertical = 2.0 * (1.0 / focal_y).atan();
            }
        }
        if let Some(clock) = jc3gi::clock::Clock::get()
            && clock.m_RealSPF > 0.0
        {
            params.frame_time_delta_ms = clock.m_RealSPF * 1000.0;
        }
        params
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clip-to-previous-clip matrix of an eye that turned by `yaw` radians and moved by
    /// `translation` metres between frames, through an infinite reverse-Z projection.
    fn eye_reprojection(yaw: f32, translation: glam::Vec3) -> Matrix4 {
        let projection = glam::Mat4::perspective_infinite_reverse_rh(1.5, 1.0, SCENE_NEAR_CLIP_M);
        let current = glam::Mat4::IDENTITY;
        let previous =
            glam::Mat4::from_rotation_translation(glam::Quat::from_rotation_y(yaw), translation);
        let current_vp = projection * current.inverse();
        let previous_vp = projection * previous.inverse();
        Matrix4::from(previous_vp * current_vp.inverse())
    }

    #[test]
    fn head_motion_is_not_a_cut_and_a_jump_is() {
        assert!(!camera_cut(&eye_reprojection(0.0, glam::Vec3::ZERO)));
        assert!(!camera_cut(&eye_reprojection(
            0.05,
            glam::Vec3::new(0.05, 0.0, 0.02)
        )));
        assert!(camera_cut(&eye_reprojection(1.2, glam::Vec3::ZERO)));
        assert!(camera_cut(&eye_reprojection(
            0.0,
            glam::Vec3::new(5.0, 0.0, 0.0)
        )));
    }

    #[test]
    fn a_side_by_side_resolve_is_one_eye_wide() {
        let whole = Build::for_layout(EyeLayout::Single(1), 640, 360);
        assert!(whole.eye_width == 640 && whole.height == 360 && !whole.side_by_side);
        let split = Build::for_layout(EyeLayout::SideBySide, 640, 360);
        assert!(split.eye_width == 320 && split.height == 360 && split.side_by_side);
        assert!(whole != split, "a layout change rebuilds the contexts");
    }

    #[test]
    fn a_still_eye_reprojects_onto_itself_and_its_centre_by_the_parallax() {
        let projection = glam::Mat4::perspective_rh(1.5, 0.9, 0.1, 1000.0);
        let center = glam::Mat4::from_translation(glam::Vec3::new(100.0, 20.0, -50.0));
        let eye = center * glam::Mat4::from_translation(glam::Vec3::new(-0.032, 0.0, 0.0));
        let frame = EyeViewProjections {
            center: projection * center.inverse(),
            eye: projection * eye.inverse(),
            jitter: glam::Vec2::ZERO,
        };
        let (to_center, to_eye) = side_by_side_reprojection(frame, frame);
        let point = glam::Vec4::new(0.25, -0.5, 0.01, 1.0);
        let same = glam::Mat4::from(to_eye) * point;
        assert!((same / same.w - point).abs().max_element() < 1e-4);
        let shifted = glam::Mat4::from(to_center) * point;
        let shift = shifted.x / shifted.w - point.x;
        assert!(
            shift.abs() > 1e-4,
            "the centre sees the point from elsewhere"
        );
        assert!((shifted.y / shifted.w - point.y).abs() < 1e-4);
    }
}
