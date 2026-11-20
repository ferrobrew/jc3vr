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

mod config;
pub use config::FsrConfig;
mod velocity_decode;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fsr_sys::{Context, DispatchInfo, Extent, init_flags};
use jc3gi::{
    graphics_engine::{device::Device, texture::Texture},
    types::math::Matrix4,
};
use parking_lot::Mutex;
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

use crate::{config::Config, hooks::camera::SCENE_NEAR_CLIP_M, stereo::STEREO_STATE};

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
    pub slot_color: &'a Texture,
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
    /// The eye's own view-projection.
    eye: glam::Mat4,
}

impl EyeViewProjections {
    /// This frame's matrices for `eye` under the single-pass collapse, where the render camera stays
    /// centred and each eye's view-projection comes from the VR frame's per-eye parameters.
    fn current(eye: usize) -> Option<Self> {
        let center = glam::Mat4::from(STEREO_STATE.lock().center_transform?);
        let params = crate::vr::render_params(eye)?;
        let projection = glam::Mat4::from(params.projection_reverse_z);
        Some(Self {
            center: projection * center.inverse(),
            eye: projection * params.resolve(center).inverse(),
        })
    }
}

/// The stereo correction's clip-to-previous-clip matrices (centre, eye) from `previous` to
/// `current`, formed in `f64` as [`crate::stereo::VpHistory::reprojection_matrices`] forms them for
/// an eye rendered on its own.
fn side_by_side_reprojection(
    previous: EyeViewProjections,
    current: EyeViewProjections,
) -> (Matrix4, Matrix4) {
    let inverse = current.eye.as_dmat4().inverse();
    (
        Matrix4::from((previous.center.as_dmat4() * inverse).as_mat4()),
        Matrix4::from((previous.eye.as_dmat4() * inverse).as_mat4()),
    )
}

/// Global FSR state. Locked briefly on the render thread at the AA hook.
pub static FSR_STATE: Mutex<FsrState> = Mutex::new(FsrState::new());

/// Discard every eye's history at the next resolve, so the accumulation restarts from that frame.
/// The visual harness calls it on every configuration, so each mode it compares accumulates from the
/// same number of frames rather than inheriting another mode's history.
/// Release both eyes' contexts, outputs, and velocity decodes, for eject. The state lives in a
/// static, which an unloading payload never drops, so without this every resource FSR built stays
/// allocated on the game's device after the payload is gone.
pub fn release_on_eject() {
    *FSR_STATE.lock() = FsrState::new();
}

pub fn request_history_reset() {
    RESET_REQUESTED.store(true, Ordering::Relaxed);
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
    if crate::is_shutting_down() {
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
    let (eyes, regions): (&[usize], [Option<u32>; 2]) = match layout {
        EyeLayout::Single(eye) => {
            let eye = eye.min(1);
            (&EYES[eye..=eye], [None, None])
        }
        EyeLayout::SideBySide => (&EYES, [Some(0), Some(build.eye_width)]),
    };
    let correction = Config::lock_query(|c| c.stereo.cameras && c.fsr.stereo_mv_correction)
        && crate::stereo::active();
    let mut ran = false;
    for &eye in eyes {
        // A side-by-side resolve takes the correction from each eye's own matrices, which the
        // collapse's centred render camera does not carry; a whole resolve takes it from the
        // render camera's history.
        let reprojection = if !correction {
            None
        } else if build.side_by_side {
            let current = EyeViewProjections::current(eye);
            let previous = std::mem::replace(&mut state.side_by_side_history[eye], current);
            previous
                .zip(current)
                .map(|(previous, current)| side_by_side_reprojection(previous, current))
        } else {
            STEREO_STATE.lock().vp_history.reprojection_matrices(eye)
        };
        if state.eyes[eye].is_none() {
            state.eyes[eye] = create_eye(device, textures.slot_color, build);
        }
        let Some(eye_state) = state.eyes[eye].as_mut() else {
            continue;
        };
        let request = EyeResolve {
            region_x: regions[eye],
            reprojection,
            build,
            sharpness,
            camera: &camera,
        };
        // SAFETY: `context.m_Mutex` guards the immediate context; every resource is valid for the
        // calls, all of which `resolve_eye` records under the mutex.
        unsafe {
            windows::Win32::System::Threading::EnterCriticalSection(context.m_Mutex);
            ran |= resolve_eye(eye_state, &context.m_Context, device, textures, &request);
            windows::Win32::System::Threading::LeaveCriticalSection(context.m_Mutex);
        }
    }
    SPLIT_DISPATCHES.store(
        if build.side_by_side { eyes.len() } else { 0 },
        Ordering::Relaxed,
    );
    ran
}

static RESET_REQUESTED: AtomicBool = AtomicBool::new(false);
static SPLIT_DISPATCHES: AtomicUsize = AtomicUsize::new(0);
static EYES: [usize; 2] = [0, 1];

/// One eye's resolve within a [`dispatch`].
struct EyeResolve<'a> {
    /// The stereo correction's matrices (centre, eye), when it applies; see
    /// [`velocity_decode::DecodeInputs::reprojection`].
    reprojection: Option<(Matrix4, Matrix4)>,
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
        reprojection,
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
            // The camera is not jittered.
            jitter: (0.0, 0.0),
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

/// The sign and axis convention applied to the decoded UV motion before FSR. The decode math
/// follows the engine's velocity encoding (see `docs/mod/rendering/fsr.md`); only FSR's expected
/// sign and Y direction is empirical, and a wrong sign is visually obvious (trails point
/// backwards). UV is Y-down, hence the flipped Y.
const MV_SIGN: (f32, f32) = (1.0, -1.0);

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
