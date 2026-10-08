//! The per-eye blit: copy each captured eye texture into its slice of the OpenXR stereo swapchain,
//! then submit the frame.
//!
//! The game renders each eye into `m_BackBufferLinear` and the mod captures it after the resolve
//! (`docs/engine/rendering/rendering.md` §12); those captures live in [`EGUI_DEBUG_RENDER_STATE`] as
//! `R8G8B8A8_UNORM` textures sized to the game's back buffer. The OpenXR swapchain is a 2-slice
//! texture array sized to the runtime's recommended per-eye resolution in a negotiated (usually
//! `_SRGB`) format. Sizes and formats generally differ, so this is a **shader blit** (fullscreen
//! triangle sampling the eye SRV into the slice RTV), not a `CopyResource`. It runs on the engine's
//! immediate context under `Context::m_Mutex`, serialized with the engine's own render work, the
//! same model as [`crate::capture::composite`].
//!
//! Each eye reaches its slice by one of two routes. The direct route ([`arm_direct`], on by default
//! through `vr.direct_eye_blit`) acquires the swapchain image at the top of the render update and
//! blits each eye from the engine's back buffer in that eye's own post-draw ([`blit_direct`]), so the
//! full-size copy into the eye capture is made only when something else reads that capture (the
//! desktop mirror's eye, a screenshot, the recorder, the previews). The capture route copies every
//! eye into its capture in the post-draw and blits the captures at the frame submit; it remains for
//! the single-pass collapse, whose one back buffer holds both eyes side by side and so has no
//! per-eye source the blit can sample whole, for a back buffer the blit cannot sample, and for any
//! eye the direct route did not reach. The submit blits exactly the eyes the direct route did not.
//!
//! The gamma bridge (linear source vs `_SRGB` target) is a runtime toggle; see
//! [`crate::vr::config::BlitGamma`] and `vr_blit_ps.hlsl`. The blit also darkens the image by the
//! comfort fade's level ([`crate::controllers::roomscale::fade`]): the engine's own screen fade
//! belongs to the game's fades (loading, teleports, cutscenes), so the mod's fade is applied here,
//! at the last stage before the compositor.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use anyhow::Context as _;
use jc3gi::graphics_engine::device::Device;
use parking_lot::Mutex;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct3D::{
                D3D_PRIMITIVE_TOPOLOGY, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
                D3D11_SRV_DIMENSION_TEXTURE2D,
            },
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC,
                D3D11_COMPARISON_NEVER, D3D11_CULL_NONE, D3D11_FILL_SOLID,
                D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_QUERY_DESC, D3D11_QUERY_EVENT,
                D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_VIEW_DESC,
                D3D11_RENDER_TARGET_VIEW_DESC_0, D3D11_RTV_DIMENSION_TEXTURE2DARRAY,
                D3D11_SAMPLER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC,
                D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_SUBRESOURCE_DATA, D3D11_TEX2D_ARRAY_RTV,
                D3D11_TEX2D_SRV, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_DEFAULT, D3D11_VIEWPORT, ID3D11Buffer, ID3D11Device,
                ID3D11DeviceContext, ID3D11DomainShader, ID3D11GeometryShader, ID3D11HullShader,
                ID3D11InputLayout, ID3D11PixelShader, ID3D11Query, ID3D11RasterizerState,
                ID3D11RenderTargetView, ID3D11Resource, ID3D11SamplerState,
                ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
            },
            Dxgi::Common::{
                DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM,
            },
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface as _,
};

use crate::{
    ui::render::EGUI_DEBUG_RENDER_STATE,
    vr::{
        EyeImage, FrameContext, VrConfig,
        config::BlitGamma,
        foveation::{capture_state, restore_state},
    },
};

/// The committed, precompiled blit shaders. The vertex shader is the shared fullscreen-triangle from
/// the capture composite (entry point `main`); the pixel shader is VR-specific (gamma bridge).
const VERTEX_DXBC: &[u8] = include_bytes!("../shaders/capture_vs.dxbc");
const PIXEL_DXBC: &[u8] = include_bytes!("../shaders/vr_blit_ps.dxbc");

/// The blit pipeline singleton, built lazily on the first submitted VR frame and torn down with the
/// runtime. Holds COM objects, which `windows` marks `Send`/`Sync`, so a `Mutex` static is sound.
static VR_BLIT: Mutex<Option<VrBlit>> = Mutex::new(None);

/// Acquire the swapchain image, blit both captured eyes into their slices, release, and submit the
/// frame. Consumes the [`FrameContext`], releasing the OpenXR runtime lock. The frame is always
/// ended -- on any blit failure it submits an empty frame rather than leaving the compositor waiting.
pub fn present_and_submit(mut frame: FrameContext, cfg: &VrConfig) {
    if !frame.should_render() {
        disarm_direct();
        end(frame);
        return;
    }

    let submitted = submit(&mut frame, cfg);
    // The frame's view over the back buffer goes whatever happened, before a resize can need the
    // buffer free.
    if let Some(blit) = VR_BLIT.lock().as_mut() {
        blit.end_frame();
    }
    if let Err(e) = submitted {
        tracing::warn!(target: "vr", "eye blit failed; submitting an empty frame: {e:#}");
        // Release any acquired image so the empty submit does not deadlock the swapchain.
        let _ = frame.release();
    }
    end(frame);
}

/// Start the direct route for this frame: acquire the swapchain image now and publish it, so each
/// eye's post-draw can blit into its slice ([`blit_direct`]). Call on the game thread after
/// [`crate::vr::frame_begin`] and before the dispatches, only for a frame that renders and whose
/// dispatches each carry one eye. Any other frame calls [`disarm_direct`] instead.
///
/// Acquiring here rather than at the submit moves the swapchain image wait onto the game thread,
/// where it is measured as `vr_swapchain_wait`; the image is the runtime's oldest and is normally
/// free already. A failed acquire leaves the route disarmed, and the submit retries the acquire and
/// blits from the captures, as it does with the route off.
pub fn arm_direct(frame: &mut FrameContext, cfg: &VrConfig) {
    let (target, arm) = if !cfg.direct_eye_blit {
        (None, DirectArm::ConfigOff)
    } else if GAVE_UP.load(Ordering::Relaxed) {
        (None, DirectArm::GaveUp)
    } else {
        match frame.acquire() {
            Ok(()) => {
                let target = frame.eye_image(0).map(|image| DirectTarget {
                    image,
                    linearize: matches!(cfg.blit_srgb_gamma, BlitGamma::Linearize),
                    blitted: [false; 2],
                });
                let arm = if target.is_some() {
                    DirectArm::Armed
                } else {
                    DirectArm::AcquireFailed
                };
                (target, arm)
            }
            Err(e) => {
                tracing::debug!(
                    target: "vr",
                    "direct eye blit: acquiring the swapchain image failed: {e:#}"
                );
                (None, DirectArm::AcquireFailed)
            }
        }
    };
    *DIRECT.lock() = target;
    LAST_ARM.store(arm as u8, Ordering::Relaxed);
}

/// Leave the direct route off for this frame, so every eye is copied into its capture and blitted
/// at the submit.
pub fn disarm_direct() {
    *DIRECT.lock() = None;
    LAST_ARM.store(DirectArm::NotArmed as u8, Ordering::Relaxed);
}

/// What the direct route did on the last frame, for a status readout. Atomics only, so any thread
/// may read it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectStatus {
    /// How the last frame's [`arm_direct`] or [`disarm_direct`] left the route, or `None` before
    /// the first frame.
    pub(crate) arm: Option<DirectArm>,
    /// How many eyes the last submitted frame took from the direct route.
    pub(crate) direct_eyes: u8,
    /// Whether the back buffer was last found readable by the direct route, or `None` before it
    /// was checked.
    pub(crate) back_buffer_readable: Option<bool>,
}

/// How a frame left the direct route.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum DirectArm {
    /// The image was acquired and published for the eyes' post-draws.
    Armed = 1,
    /// `vr.direct_eye_blit` is off.
    ConfigOff,
    /// An earlier failure turned the route off for the session.
    GaveUp,
    /// Acquiring the swapchain image failed this frame.
    AcquireFailed,
    /// The frame did not ask for the route: it does not render, does not render in stereo, or its
    /// one dispatch carries both eyes.
    NotArmed,
}

/// The direct route's last frame.
pub(crate) fn direct_status() -> DirectStatus {
    let arm = match LAST_ARM.load(Ordering::Relaxed) {
        1 => Some(DirectArm::Armed),
        2 => Some(DirectArm::ConfigOff),
        3 => Some(DirectArm::GaveUp),
        4 => Some(DirectArm::AcquireFailed),
        5 => Some(DirectArm::NotArmed),
        _ => None,
    };
    DirectStatus {
        arm,
        direct_eyes: LAST_DIRECT_EYES.load(Ordering::Relaxed),
        back_buffer_readable: match BACK_BUFFER_READABLE.load(Ordering::Relaxed) {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        },
    }
}

/// Blit `src`, the engine's back buffer as eye `eye`'s post-draw leaves it, into that eye's
/// swapchain slice, when the direct route is armed for this frame. Returns whether it did; on
/// `false` the caller copies the eye into its capture, so the submit blits it from there. Restores
/// the pipeline state the engine's following passes rely on.
///
/// # Safety
/// Called from the post-draw hook with the engine context mutex held; `device` and `context` are
/// the live engine device and immediate context.
pub(crate) unsafe fn blit_direct(
    device: &Device,
    context: &ID3D11DeviceContext,
    src: &ID3D11Resource,
    eye: usize,
) -> bool {
    if eye >= 2 {
        return false;
    }
    let mut direct = DIRECT.lock();
    let Some(target) = direct.as_mut() else {
        return false;
    };
    let Ok(src) = src.cast::<ID3D11Texture2D>() else {
        return false;
    };
    let brightness = 1.0 - crate::controllers::roomscale::fade::level();
    let image = EyeImage {
        array_index: eye as u32,
        ..target.image
    };

    let mut guard = VR_BLIT.lock();
    if guard.is_none() {
        match VrBlit::new(device) {
            Ok(blit) => *guard = Some(blit),
            Err(e) => {
                give_up(format_args!("building the blit pipeline failed: {e:#}"));
                return false;
            }
        }
    }
    let blit = guard.as_mut().expect("the blit pipeline was just ensured");
    let Some(srv) = blit.back_buffer_srv(device, &src) else {
        return false;
    };
    // SAFETY: the caller holds the context mutex; the image stays acquired until the submit
    // releases it, which takes the target out of `DIRECT` first.
    let result = unsafe {
        let targets = capture_state(context);
        blit.set_params(context, target.linearize, brightness);
        let result = blit.draw_eye(device, context, &image, srv);
        restore_state(context, &targets);
        result
    };
    match result {
        Ok(()) => {
            target.blitted[eye] = true;
            true
        }
        Err(e) => {
            give_up(format_args!("the draw failed: {e:#}"));
            false
        }
    }
}

/// Tear down the blit pipeline (COM release). Called from the runtime cleanup.
pub fn teardown() {
    *DIRECT.lock() = None;
    *VR_BLIT.lock() = None;
}

/// The swapchain image the frame's dispatches blit into on the direct route, and which eyes they
/// have reached. Present from [`arm_direct`] until the submit takes it, before it releases the
/// image.
struct DirectTarget {
    /// The acquired image; each blit picks its eye's slice.
    image: EyeImage,
    linearize: bool,
    blitted: [bool; 2],
}

// SAFETY: the raw pointer is the runtime-owned swapchain texture, valid from the acquire in
// `arm_direct` until the release in `submit`, which takes the target out of `DIRECT` first; every
// use of it is on the engine's immediate context under its mutex.
unsafe impl Send for DirectTarget {}

/// The direct route's target for the frame in flight, or `None` while the route is off.
static DIRECT: Mutex<Option<DirectTarget>> = Mutex::new(None);

/// Whether the back buffer was last found readable by the direct route: `0` no, `1` yes, `2` not
/// checked yet. Only for logging the verdict when it changes.
static BACK_BUFFER_READABLE: AtomicU8 = AtomicU8::new(2);

/// The last frame's [`DirectArm`] as its discriminant, `0` before the first frame.
static LAST_ARM: AtomicU8 = AtomicU8::new(0);

/// How many eyes the last submitted frame took from the direct route.
static LAST_DIRECT_EYES: AtomicU8 = AtomicU8::new(0);

/// Set when the direct route failed in a way a later frame would repeat, which leaves it off for
/// the rest of the session.
static GAVE_UP: AtomicBool = AtomicBool::new(false);

/// Turn the direct route off for the session after a failure, logging it once. The eye that failed
/// is copied into its capture and blitted at the submit like every eye after it.
fn give_up(reason: std::fmt::Arguments<'_>) {
    if !GAVE_UP.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            target: "vr",
            "direct eye blit: {reason}; copying the eyes into their captures for the rest of \
             the session"
        );
    }
}

/// The shader-stage bindings the blit changes, which [`capture_state`] does not cover: the input
/// assembler's, the vertex and pixel shaders, the geometry, hull, and domain shaders it unbinds,
/// the scissor rectangles, and the pixel shader's first sampler, constant buffer, and resource.
/// Every blit restores them: one inside the engine's frame is followed by the engine's own
/// drawing, and one at the submit by the next frame's.
struct ShaderBindings {
    input_layout: Option<ID3D11InputLayout>,
    topology: D3D_PRIMITIVE_TOPOLOGY,
    vertex_shader: Option<ID3D11VertexShader>,
    pixel_shader: Option<ID3D11PixelShader>,
    geometry_shader: Option<ID3D11GeometryShader>,
    hull_shader: Option<ID3D11HullShader>,
    domain_shader: Option<ID3D11DomainShader>,
    scissors: Vec<RECT>,
    ps_samplers: [Option<ID3D11SamplerState>; 1],
    ps_constants: [Option<ID3D11Buffer>; 1],
    ps_resources: [Option<ID3D11ShaderResourceView>; 1],
}

impl ShaderBindings {
    /// # Safety
    /// `context` must be the live engine immediate context, held under its mutex.
    unsafe fn capture(context: &ID3D11DeviceContext) -> Self {
        let mut bindings = Self {
            input_layout: None,
            topology: D3D_PRIMITIVE_TOPOLOGY::default(),
            vertex_shader: None,
            pixel_shader: None,
            geometry_shader: None,
            hull_shader: None,
            domain_shader: None,
            scissors: Vec::new(),
            ps_samplers: Default::default(),
            ps_constants: Default::default(),
            ps_resources: Default::default(),
        };
        // SAFETY: every out-pointer is a live local; the getters AddRef what they return, which the
        // `Option` wrappers release.
        unsafe {
            bindings.input_layout = context.IAGetInputLayout().ok();
            bindings.topology = context.IAGetPrimitiveTopology();
            context.VSGetShader(&mut bindings.vertex_shader, None, None);
            context.PSGetShader(&mut bindings.pixel_shader, None, None);
            context.GSGetShader(&mut bindings.geometry_shader, None, None);
            context.HSGetShader(&mut bindings.hull_shader, None, None);
            context.DSGetShader(&mut bindings.domain_shader, None, None);
            let mut count = 0;
            context.RSGetScissorRects(&mut count, None);
            bindings.scissors = vec![RECT::default(); count as usize];
            if count > 0 {
                context.RSGetScissorRects(&mut count, Some(bindings.scissors.as_mut_ptr()));
                bindings.scissors.truncate(count as usize);
            }
            context.PSGetSamplers(0, Some(&mut bindings.ps_samplers));
            context.PSGetConstantBuffers(0, Some(&mut bindings.ps_constants));
            context.PSGetShaderResources(0, Some(&mut bindings.ps_resources));
        }
        bindings
    }

    /// # Safety
    /// `context` must be the immediate context the bindings were captured from, held under its
    /// mutex.
    unsafe fn restore(self, context: &ID3D11DeviceContext) {
        // SAFETY: rebinding objects the context itself returned.
        unsafe {
            context.IASetInputLayout(self.input_layout.as_ref());
            context.IASetPrimitiveTopology(self.topology);
            context.VSSetShader(self.vertex_shader.as_ref(), None);
            context.PSSetShader(self.pixel_shader.as_ref(), None);
            context.GSSetShader(self.geometry_shader.as_ref(), None);
            context.HSSetShader(self.hull_shader.as_ref(), None);
            context.DSSetShader(self.domain_shader.as_ref(), None);
            context.RSSetScissorRects(Some(&self.scissors));
            context.PSSetSamplers(0, Some(&self.ps_samplers));
            context.PSSetConstantBuffers(0, Some(&self.ps_constants));
            context.PSSetShaderResources(0, Some(&self.ps_resources));
        }
    }
}

/// The maximum time to wait for the GPU to report idle during teardown, in ~1 ms polling steps. A
/// hard cap so a teardown can never itself hang if the pipeline is already wedged; the drain is
/// best-effort.
const DRAIN_MAX_POLLS: u32 = 2000;

/// Flush the engine's immediate context and wait (bounded) for the GPU to go idle. Called at the top
/// of the VR runtime teardown, *before* the XR swapchain and session are destroyed: the per-eye blit
/// writes into the swapchain images and the last submitted frame may still be in flight, so tearing
/// the swapchain down underneath that work strands the DXVK/present pipeline. The game's own `Flip`
/// then deadlocks in a timestamp-query `GetData` that never completes — while it holds the engine
/// context mutex, which freezes every other thread behind it. Draining here lets that work retire so
/// the game resumes cleanly. Best-effort: any failure (or the poll budget expiring) logs and returns;
/// teardown must never hang or panic.
pub fn drain_gpu() {
    let Some(ge) = (unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() })
    else {
        return;
    };
    // Drain the CPU-side draw dispatch first, so its command list is submitted before we wait on the
    // GPU (`WaitForCPUDrawToFinish` is CPU-side only).
    unsafe { ge.WaitForCPUDrawToFinish() };
    let Some(device) = (unsafe { ge.m_Device.as_ref() }) else {
        return;
    };
    let Some(context) = (unsafe { device.m_Context.as_ref() }) else {
        return;
    };
    unsafe {
        EnterCriticalSection(context.m_Mutex);
        let result = drain_immediate(&device.m_Device, &context.m_Context);
        LeaveCriticalSection(context.m_Mutex);
        if let Err(e) = result {
            tracing::warn!(target: "vr", "GPU drain during teardown was incomplete: {e:#}");
        }
    }
}

/// Submit everything pending and poll an event query until the GPU signals it, up to
/// [`DRAIN_MAX_POLLS`]. Held under the context mutex by the caller.
unsafe fn drain_immediate(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
) -> anyhow::Result<()> {
    let desc = D3D11_QUERY_DESC {
        Query: D3D11_QUERY_EVENT,
        MiscFlags: 0,
    };
    let mut query: Option<ID3D11Query> = None;
    unsafe { device.CreateQuery(&desc, Some(&mut query)) }
        .context("creating the drain event query")?;
    let query = query.context("the drain event query was not created")?;
    unsafe {
        context.End(&query);
        context.Flush();
    }
    // `GetData` returns S_OK once the GPU has passed the `End`, S_FALSE while still pending. The
    // safe `GetData` wrapper folds S_FALSE into `Ok`, so it cannot distinguish the two; call the raw
    // vtable entry for the real HRESULT. The cap guarantees the loop terminates even if the pipeline
    // never drains.
    let vtable = context.vtable();
    for _ in 0..DRAIN_MAX_POLLS {
        let hr = unsafe {
            (vtable.GetData)(context.as_raw(), query.as_raw(), std::ptr::null_mut(), 0, 0)
        };
        if hr == windows::Win32::Foundation::S_OK {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    anyhow::bail!("the GPU did not report idle within the teardown budget")
}

/// End the frame, logging a submit failure (the frame is consumed regardless).
fn end(frame: FrameContext) {
    if let Err(e) = frame.frame_end() {
        tracing::warn!(target: "vr", "frame end failed: {e:#}");
    }
}

/// The acquire → blit → release body, factored out so [`present_and_submit`] can end the frame on
/// either success or failure.
fn submit(frame: &mut FrameContext, cfg: &VrConfig) -> anyhow::Result<()> {
    // Clone (AddRef) the captured eye textures under a brief EGUI lock, dropped before the blit
    // state / context work, mirroring `capture::present_active`'s lock discipline.
    let eye_textures: [Option<ID3D11Texture2D>; 2] = {
        let lock = EGUI_DEBUG_RENDER_STATE.lock();
        [lock.texture(0).cloned(), lock.texture(1).cloned()]
    };
    // Every dispatch of the frame is drained by now, so no post-draw reads the target again; taking
    // it here, before the release below, is what keeps a later frame off a released image.
    let blitted = DIRECT
        .lock()
        .take()
        .map_or([false; 2], |target| target.blitted);
    LAST_DIRECT_EYES.store(
        blitted.iter().filter(|&&eye| eye).count() as u8,
        Ordering::Relaxed,
    );

    frame
        .acquire()
        .context("vr: acquiring the swapchain image")?;

    let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        .context("vr: the graphics engine is unavailable for the blit")?;
    let device =
        unsafe { ge.m_Device.as_ref() }.context("vr: the graphics device is unavailable")?;
    let context =
        unsafe { device.m_Context.as_ref() }.context("vr: the graphics context is unavailable")?;

    let linearize = matches!(cfg.blit_srgb_gamma, BlitGamma::Linearize);
    let brightness = 1.0 - crate::controllers::roomscale::fade::level();

    let mut guard = VR_BLIT.lock();
    if guard.is_none() {
        *guard = Some(VrBlit::new(device)?);
    }
    let blit = guard.as_mut().expect("the blit pipeline was just ensured");

    // Both eyes' immediate-context work runs under one critical-section hold, serialized with the
    // engine's render work.
    unsafe {
        EnterCriticalSection(context.m_Mutex);
        blit.set_params(&context.m_Context, linearize, brightness);
        for (eye, src) in eye_textures.iter().enumerate() {
            if blitted[eye] {
                continue;
            }
            let (Some(image), Some(src)) = (frame.eye_image(eye), src.as_ref()) else {
                continue;
            };
            if let Err(e) = blit.blit_eye(device, &context.m_Context, &image, src) {
                tracing::warn!(target: "vr", eye, "eye blit draw failed: {e:#}");
            }
        }
        LeaveCriticalSection(context.m_Mutex);
    }
    drop(guard);

    frame
        .release()
        .context("vr: releasing the swapchain image")?;
    Ok(())
}

/// The blit pipeline: shaders, sampler, rasterizer, parameter constant buffer, and caches of the
/// per-slice render-target views and per-eye source shader-resource views.
struct VrBlit {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    rasterizer: ID3D11RasterizerState,
    /// The parameter constant buffer (`b0`): the gamma mode and the comfort fade's brightness,
    /// updated per submit.
    params_cb: ID3D11Buffer,
    /// Render-target views over swapchain slices, keyed by `(texture pointer, array slice)`. The
    /// swapchain images are enumerated once and stable, so this cache is valid for the runtime's
    /// lifetime.
    rtv_cache: Vec<(usize, u32, ID3D11RenderTargetView)>,
    /// Source shader-resource views over the captured eye textures, keyed by texture pointer, so a
    /// resize (which rebuilds the capture textures) rebuilds the SRV.
    srv_cache: [Option<(usize, ID3D11ShaderResourceView)>; 2],
    /// The frame's view over the engine's back buffer for the direct route, keyed by texture
    /// pointer, dropped at the submit.
    back_buffer_srv: Option<(usize, ID3D11ShaderResourceView)>,
}

impl VrBlit {
    fn new(device: &Device) -> anyhow::Result<Self> {
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors are valid for these calls.
        unsafe {
            let mut vertex_shader: Option<ID3D11VertexShader> = None;
            d3d.CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("vr: creating the blit vertex shader")?;
            let vertex_shader =
                vertex_shader.context("vr: the blit vertex shader was not created")?;

            let mut pixel_shader: Option<ID3D11PixelShader> = None;
            d3d.CreatePixelShader(PIXEL_DXBC, None, Some(&mut pixel_shader))
                .context("vr: creating the blit pixel shader")?;
            let pixel_shader = pixel_shader.context("vr: the blit pixel shader was not created")?;

            let mut sampler: Option<ID3D11SamplerState> = None;
            d3d.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MinLOD: f32::MIN,
                    MaxLOD: f32::MAX,
                    MipLODBias: 0.0,
                    MaxAnisotropy: 0,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    BorderColor: [0.0; 4],
                },
                Some(&mut sampler),
            )
            .context("vr: creating the blit sampler")?;
            let sampler = sampler.context("vr: the blit sampler was not created")?;

            let mut rasterizer: Option<ID3D11RasterizerState> = None;
            d3d.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )
            .context("vr: creating the blit rasterizer state")?;
            let rasterizer = rasterizer.context("vr: the blit rasterizer state was not created")?;

            // 16-byte constant buffer: the gamma mode as a uint, then the brightness as a float; the
            // rest is padding.
            let initial: [u32; 4] = [1, 1.0f32.to_bits(), 0, 0];
            let mut params_cb: Option<ID3D11Buffer> = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: std::mem::size_of::<[u32; 4]>() as u32,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: initial.as_ptr().cast(),
                    SysMemPitch: 0,
                    SysMemSlicePitch: 0,
                }),
                Some(&mut params_cb),
            )
            .context("vr: creating the blit parameter constant buffer")?;
            let params_cb =
                params_cb.context("vr: the blit parameter constant buffer was not created")?;

            Ok(Self {
                vertex_shader,
                pixel_shader,
                sampler,
                rasterizer,
                params_cb,
                rtv_cache: Vec::new(),
                srv_cache: [None, None],
                back_buffer_srv: None,
            })
        }
    }

    /// Update the parameter constant buffer: the gamma mode and the comfort fade's brightness
    /// (`0..=1`). The caller must hold the context mutex.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context.
    unsafe fn set_params(&self, context: &ID3D11DeviceContext, linearize: bool, brightness: f32) {
        let brightness = if brightness.is_finite() {
            brightness.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let data: [u32; 4] = [u32::from(linearize), brightness.to_bits(), 0, 0];
        unsafe {
            context.UpdateSubresource(&self.params_cb, 0, None, data.as_ptr().cast(), 0, 0);
        }
    }

    /// Blit one captured eye into its swapchain slice. The caller must hold the context mutex.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context; `image` must reference the currently
    /// acquired swapchain texture and `src` a live capture texture.
    unsafe fn blit_eye(
        &mut self,
        device: &Device,
        context: &ID3D11DeviceContext,
        image: &EyeImage,
        src: &ID3D11Texture2D,
    ) -> anyhow::Result<()> {
        let srv = self.srv_for(device, image.array_index as usize, src)?;
        unsafe { self.draw_eye(device, context, image, srv) }
    }

    /// Draw `srv` over `image`'s swapchain slice. The caller must hold the context mutex.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context; `image` must reference the currently
    /// acquired swapchain texture.
    unsafe fn draw_eye(
        &mut self,
        device: &Device,
        context: &ID3D11DeviceContext,
        image: &EyeImage,
        srv: ID3D11ShaderResourceView,
    ) -> anyhow::Result<()> {
        // Wrap the runtime-owned swapchain texture borrowed (no AddRef); do not release it.
        let swapchain_tex = unsafe { ID3D11Texture2D::from_raw_borrowed(&image.texture) }
            .context("vr: the swapchain texture pointer was null")?;

        let (width, height) = unsafe { texture_size(swapchain_tex) };
        if width == 0 || height == 0 {
            return Ok(());
        }

        let rtv = self.rtv_for(device, swapchain_tex, image)?;

        unsafe {
            let bindings = ShaderBindings::capture(context);
            context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: width as f32,
                Height: height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            // Opaque: the slice takes the eye image as it is, whatever blend state is bound.
            context.OMSetBlendState(None, None, u32::MAX);
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            // A geometry, hull, or domain shader the engine left bound would sit between the
            // full-screen triangle and the pixel shader; the scissor is set to the slice in case a
            // rasterizer state with scissoring is ever bound here.
            context.GSSetShader(None, None);
            context.HSSetShader(None, None);
            context.DSSetShader(None, None);
            context.RSSetScissorRects(Some(&[RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            }]));
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.params_cb.clone())]));
            context.PSSetShaderResources(0, Some(&[Some(srv)]));
            context.RSSetState(&self.rasterizer);
            context.Draw(3, 0);

            // Unbind so the engine's next pass does not see our SRV/RTV still bound.
            context.PSSetShaderResources(0, Some(&[None]));
            context.OMSetRenderTargets(Some(&[None]), None);
            bindings.restore(context);
        }
        Ok(())
    }

    /// Get (creating and caching on first use) the render-target view over `image`'s swapchain slice.
    fn rtv_for(
        &mut self,
        device: &Device,
        swapchain_tex: &ID3D11Texture2D,
        image: &EyeImage,
    ) -> anyhow::Result<ID3D11RenderTargetView> {
        let key = (image.texture as usize, image.array_index);
        if let Some((_, _, rtv)) = self
            .rtv_cache
            .iter()
            .find(|(p, s, _)| *p == key.0 && *s == key.1)
        {
            return Ok(rtv.clone());
        }
        let desc = D3D11_RENDER_TARGET_VIEW_DESC {
            Format: DXGI_FORMAT(image.format as i32),
            ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2DARRAY,
            Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 {
                Texture2DArray: D3D11_TEX2D_ARRAY_RTV {
                    MipSlice: 0,
                    FirstArraySlice: image.array_index,
                    ArraySize: 1,
                },
            },
        };
        let mut rtv: Option<ID3D11RenderTargetView> = None;
        unsafe {
            device
                .m_Device
                .CreateRenderTargetView(swapchain_tex, Some(&desc), Some(&mut rtv))
        }
        .context("vr: creating the swapchain slice render-target view")?;
        let rtv = rtv.context("vr: the swapchain slice render-target view was not created")?;
        self.rtv_cache.push((key.0, key.1, rtv.clone()));
        Ok(rtv)
    }

    /// Get (creating and caching on first use, rebuilding on a texture change) the source
    /// shader-resource view over the captured eye texture for `eye`.
    fn srv_for(
        &mut self,
        device: &Device,
        eye: usize,
        src: &ID3D11Texture2D,
    ) -> anyhow::Result<ID3D11ShaderResourceView> {
        let ptr = src.as_raw() as usize;
        let slot = &mut self.srv_cache[eye.min(1)];
        if let Some((cached, srv)) = slot.as_ref()
            && *cached == ptr
        {
            return Ok(srv.clone());
        }
        let mut srv: Option<ID3D11ShaderResourceView> = None;
        unsafe {
            device
                .m_Device
                .CreateShaderResourceView(src, None, Some(&mut srv))
        }
        .context("vr: creating the captured eye shader-resource view")?;
        let srv = srv.context("vr: the captured eye shader-resource view was not created")?;
        *slot = Some((ptr, srv.clone()));
        Ok(srv)
    }

    /// Get (creating on the frame's first use) a shader-resource view over the engine's back buffer
    /// that samples it exactly as its copy into an eye capture does: the raw 8-bit values, through
    /// an `R8G8B8A8_UNORM` view. `None` when the texture cannot be read that way (no shader-resource
    /// binding, multisampled, or a format other than `R8G8B8A8_UNORM` or `R8G8B8A8_TYPELESS`); the
    /// caller then takes the capture route. The verdict is logged when it changes.
    ///
    /// The view holds a reference to the back buffer, which without the mod-owned back buffer is
    /// DXGI buffer 0, and a swapchain resize fails while any reference to it is outstanding; so the
    /// view lives for one frame only, dropped by [`VrBlit::end_frame`] at the submit.
    fn back_buffer_srv(
        &mut self,
        device: &Device,
        src: &ID3D11Texture2D,
    ) -> Option<ID3D11ShaderResourceView> {
        let ptr = src.as_raw() as usize;
        if let Some((cached, srv)) = &self.back_buffer_srv
            && *cached == ptr
        {
            return Some(srv.clone());
        }
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `src` is a live texture.
        unsafe { src.GetDesc(&mut desc) };
        let readable = desc.BindFlags & D3D11_BIND_SHADER_RESOURCE.0 as u32 != 0
            && desc.SampleDesc.Count == 1
            && (desc.Format == DXGI_FORMAT_R8G8B8A8_UNORM
                || desc.Format == DXGI_FORMAT_R8G8B8A8_TYPELESS);
        if BACK_BUFFER_READABLE.swap(u8::from(readable), Ordering::Relaxed) != u8::from(readable) {
            if readable {
                tracing::info!(
                    target: "vr",
                    width = desc.Width,
                    height = desc.Height,
                    "direct eye blit: blitting the eyes from the back buffer",
                );
            } else {
                tracing::info!(
                    target: "vr",
                    format = desc.Format.0,
                    bind_flags = desc.BindFlags,
                    samples = desc.SampleDesc.Count,
                    "direct eye blit: the back buffer cannot be sampled as the eye captures are; \
                     copying the eyes instead",
                );
            }
        }
        if !readable {
            return None;
        }
        let view = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                },
            },
        };
        let mut srv: Option<ID3D11ShaderResourceView> = None;
        // SAFETY: `src` has the shader-resource binding and a format this view can read.
        if let Err(e) = unsafe {
            device
                .m_Device
                .CreateShaderResourceView(src, Some(&view), Some(&mut srv))
        } {
            give_up(format_args!("creating the back buffer's view failed: {e}"));
            return None;
        }
        let srv = srv?;
        self.back_buffer_srv = Some((ptr, srv.clone()));
        Some(srv)
    }

    /// Drop the frame's view over the back buffer (see [`VrBlit::back_buffer_srv`]).
    fn end_frame(&mut self) {
        self.back_buffer_srv = None;
    }
}

/// The pixel dimensions of a texture from its descriptor.
///
/// # Safety
/// `texture` must be a live `ID3D11Texture2D`.
unsafe fn texture_size(texture: &ID3D11Texture2D) -> (u32, u32) {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.GetDesc(&mut desc);
    }
    (desc.Width, desc.Height)
}
