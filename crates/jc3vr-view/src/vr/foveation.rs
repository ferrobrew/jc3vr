//! Static foveated rendering (issue #29): drop a dithered fraction of the peripheral pixels before the
//! expensive scene shading passes run, then reconstruct them, so the GPU shades fewer fragments where the
//! HMD's optics blur detail anyway.
//!
//! Three cooperating pieces, driven from `jc3vr_render::hooks::graphics_engine::render_pass`:
//!
//! 1. **Mask-write** ([`mask_write`]): a full-screen pass over the main depth-stencil surface that tags a
//!    radial, dithered set of peripheral pixels with a free stencil bit (the mask bit), via a
//!    REPLACE-on-pass depth-stencil state. `foveation_mask_ps.hlsl` `discard`s the pixels to keep at full
//!    resolution, so only the dropped ones receive the bit. Runs once per eye, after the depth prepass has
//!    cleared stencil and before the foveated shading range.
//! 2. **Force-test** ([`apply_force_test`]): while the foveated shading passes run, the game's own
//!    `SetupRenderStates` is intercepted to inject a stencil *test* into each draw's packed depth-stencil
//!    index (`EQUAL 0` against the mask bit), so the GPU skips the tagged peripheral fragments. Only draws
//!    that don't already use stencil are touched, and only their stencil sub-index is rewritten, leaving
//!    their depth test intact.
//! 3. **Fill-in** ([`fill_in`]): a full-screen pass over the main colour buffer that reconstructs each
//!    dropped pixel from the average of its kept neighbours (`foveation_fill_ps.hlsl`), reading a copy of
//!    the colour buffer taken just before. Runs once per eye, after the last foveated pass
//!    ([`FoveatedPasses::fill_before`]).
//!
//! [`FoveatedPasses`] names the passes the test is forced through: always the models, the vegetation,
//! and the creatures, and by default the G-buffer and lighting passes that write only their own pixels.
//!
//! The packed-index bit layout is documented on `jc3gi`'s `HContext_t::m_DepthStencilStateIndex`. The two
//! D3D passes borrow the engine's immediate context under `Context::m_Mutex`, the same discipline as
//! [`crate::vr::blit`] and [`crate::vr::mirror`]. On by default, but still experimental;
//! disabling it skips the two passes and the per-draw index rewrite; see
//! [`crate::vr::FoveationConfig`].

use std::{
    ffi::c_void,
    ptr::addr_of,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use anyhow::Context as _;
use jc3gi::graphics_engine::{
    device::Device, graphics_engine::HContext_t, render_engine::RenderPassId,
};
use parking_lot::Mutex;
use windows::{
    Win32::{
        Graphics::{
            Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC,
                D3D11_COMPARISON_ALWAYS, D3D11_COMPARISON_EQUAL, D3D11_CULL_NONE,
                D3D11_DEPTH_STENCIL_DESC, D3D11_DEPTH_STENCILOP_DESC, D3D11_DEPTH_WRITE_MASK_ZERO,
                D3D11_FILL_SOLID, D3D11_RASTERIZER_DESC, D3D11_STENCIL_OP_KEEP,
                D3D11_STENCIL_OP_REPLACE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
                D3D11_VIEWPORT, ID3D11BlendState, ID3D11Buffer, ID3D11DepthStencilState,
                ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader,
                ID3D11RasterizerState, ID3D11RenderTargetView, ID3D11ShaderResourceView,
                ID3D11Texture2D, ID3D11VertexShader,
            },
            Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC},
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface as _,
};

/// Raised by the render-pass hook for exactly the foveated shading range, so [`apply_force_test`] injects
/// the peripheral stencil test only into those draws.
pub static FORCE_STENCIL_TEST: AtomicBool = AtomicBool::new(false);

/// The precomputed stencil-test bits to OR into a draw's packed depth-stencil index while
/// [`FORCE_STENCIL_TEST`] is set, or `0` when foveation is inactive. Published once per frame by the hook
/// so the hot per-draw path is two atomic loads, not a config-mutex acquire.
pub static STENCIL_TEST_BITS: AtomicU64 = AtomicU64::new(0);

/// The bits of a packed depth-stencil index that the stencil test occupies (`StencilEnable` through
/// `StencilPassOp`, low-dword bits 6..31 and high-dword bits 32..39). Cleared before OR-ing in
/// [`STENCIL_TEST_BITS`] so the injected test fully replaces any prior stencil config while leaving the
/// depth bits (0..5) untouched.
const STENCIL_INDEX_MASK: u64 = 0x0000_00FF_7FFF_FFC0;

/// The `StencilEnable` bit (bit 6) of a packed depth-stencil index; a draw that already has it set uses
/// stencil for its own purpose and is left alone.
const STENCIL_ENABLE_BIT: u64 = 1 << 6;

/// Foveal radius as a fraction of an eye's half-diagonal from its foveal centre: inside it the scene
/// renders at full rate.
pub const INNER_FRACTION: f32 = 0.35;

/// The radius, in the units of [`INNER_FRACTION`], at which the peripheral drop reaches
/// [`MAX_DROP`]; the drop ramps from zero at the inner radius to the maximum here.
pub const OUTER_FRACTION: f32 = 0.55;

/// The fraction of peripheral pixels dropped (and reconstructed) at [`OUTER_FRACTION`] and beyond.
pub const MAX_DROP: f32 = 0.8;

/// The stencil bit the mask tags dropped pixels with: bit 7, which the engine's own stencil use
/// leaves free.
pub const MASK_BIT: u32 = 0x80;

/// Parameters for the mask-write and fill-in passes, in surface-independent units; the passes
/// convert them to pixels against the live main-buffer size.
#[derive(Clone, Copy)]
pub struct FoveationParams {
    /// How the eyes are laid out on the target the passes run over.
    pub layout: EyeLayout,
    /// Foveal radius as a fraction of an eye's half-diagonal: inside it nothing is dropped.
    pub inner_fraction: f32,
    /// Radius (fraction of the half-diagonal) at which the drop reaches [`max_drop`](Self::max_drop).
    pub outer_fraction: f32,
    /// Maximum fraction of peripheral pixels dropped.
    pub max_drop: f32,
    /// The free stencil bit tagged on dropped pixels.
    pub mask_bit: u32,
    /// Diagnostic: the fill-in pass paints the dropped set magenta instead of reconstructing it.
    pub debug_show_mask: bool,
    /// The side of a drop cell in pixels: 1 drops single pixels, 2 drops whole 2 by 2 pixel quads.
    pub cell_px: u32,
}

/// Where the eyes lie on the target a foveation pass runs over, each with its foveal centre as a UV in
/// `[0, 1]` of the eye's own region (the eye's principal point; `(0.5, 0.5)` is the region's centre).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EyeLayout {
    /// The target holds one eye.
    Single { center_uv: glam::Vec2 },
    /// The target holds both eyes side by side, eye 0 in the left half (the single-pass collapse's
    /// double-wide target): each half is masked and filled as an eye-sized target would be.
    SideBySide { center_uv: [glam::Vec2; 2] },
}

/// The pass constants for `params` on a `width` x `height` target, matching the shaders'
/// `FoveationParams`: both eyes' centres in eye-local pixels, the inner and outer radii in pixels
/// (fractions of an eye's half-diagonal), the maximum drop, the debug flag, the first column of eye 1's
/// half (past the target when it holds one eye), and the drop cell's side, padded to a whole number of
/// registers.
pub fn pass_constants(params: &FoveationParams, width: u32, height: u32) -> [f32; 12] {
    let (eye_width, centers, split) = match params.layout {
        EyeLayout::Single { center_uv } => (width, [center_uv; 2], width as f32 + 1.0),
        EyeLayout::SideBySide { center_uv } => (width / 2, center_uv, (width / 2) as f32),
    };
    let (eye_width, height) = (eye_width as f32, height as f32);
    let half_diag = 0.5 * (eye_width.powi(2) + height.powi(2)).sqrt();
    [
        centers[0].x * eye_width,
        centers[0].y * height,
        centers[1].x * eye_width,
        centers[1].y * height,
        params.inner_fraction * half_diag,
        params.outer_fraction * half_diag,
        params.max_drop,
        if params.debug_show_mask { 1.0 } else { 0.0 },
        split,
        params.cell_px as f32,
        0.0,
        0.0,
    ]
}

/// Which render passes the peripheral stencil test is forced through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoveatedPasses {
    /// Add the G-buffer passes that write only their own pixels and leave depth to the prepasses:
    /// the terrain's apply passes, the road junctions, and the decals
    /// (`single_pass.foveation_gbuffer`).
    pub gbuffer: bool,
    /// Add the lighting passes that write only their own pixels, from the deferred lights through
    /// the fog gradient, and move the fill-in to just after them (`single_pass.foveation_lighting`).
    pub lighting: bool,
}

impl FoveatedPasses {
    /// Whether the peripheral stencil test is forced through `pass`.
    #[inline]
    pub fn contains(self, pass: i32) -> bool {
        let within =
            |&(lo, hi): &(RenderPassId, RenderPassId)| (lo as i32..=hi as i32).contains(&pass);
        within(&BASE_FOVEATED_PASSES)
            || (self.gbuffer && GBUFFER_FOVEATED_PASSES.iter().any(within))
            || (self.lighting && LIGHTING_FOVEATED_PASSES.iter().any(within))
    }

    /// The first pass the test is forced through: the mask-write runs just before it.
    #[inline]
    pub fn first(self) -> i32 {
        (0..RenderPassId::RP_RENDERPASS_COUNT as i32)
            .find(|&pass| self.contains(pass))
            .expect("the base passes are always foveated")
    }

    /// The pass the fill-in runs just before, or `None` when it runs at the start of the post
    /// effects.
    pub fn fill_before(self) -> Option<i32> {
        self.lighting
            .then_some(RenderPassId::RP_FOG_GRADIENT as i32 + 1)
    }
}

/// The passes the test is always forced through (inclusive): the models, the vegetation, and the
/// creatures.
const BASE_FOVEATED_PASSES: (RenderPassId, RenderPassId) =
    (RenderPassId::RP_MODELS_DYNAMIC, RenderPassId::RP_CREATURES);

/// The G-buffer passes [`FoveatedPasses::gbuffer`] adds (inclusive ranges). Each writes the G-buffer
/// at its own pixels and no depth the prepasses have not already written, so a dropped pixel keeps
/// its depth for every later reader (the SSAO, the depth downsample, the fog, the water's depth test,
/// and FSR). The terrain's detail and base-mesh passes are left out: they write depth the prepasses
/// do not, and a dropped pixel without it changes the kept pixels that read its neighbourhood.
const GBUFFER_FOVEATED_PASSES: [(RenderPassId, RenderPassId); 3] = [
    (
        RenderPassId::RP_TERRAIN_APPLY_NEAR_DETAILED,
        RenderPassId::RP_TERRAIN_APPLY_FAR,
    ),
    (
        RenderPassId::RP_ROAD_JUNCTION,
        RenderPassId::RP_ROAD_JUNCTION_OPAQUE,
    ),
    (
        RenderPassId::RP_DECALS,
        RenderPassId::RP_SCREEN_SPACE_ROAD_DECALS,
    ),
];

/// The lighting passes [`FoveatedPasses::lighting`] adds (inclusive ranges). Each writes the lit
/// colour at its own pixels from that pixel's inputs alone, and nothing reads the lit colour's
/// neighbourhood until the fill-in has run after the last of them. The occlusion queries between
/// them stay out: a dropped pixel would no longer count towards a query's visible samples.
const LIGHTING_FOVEATED_PASSES: [(RenderPassId, RenderPassId); 4] = [
    (
        RenderPassId::RP_DEFERRED_LIGHTS,
        RenderPassId::RP_DEFERRED_LIGHTS,
    ),
    (RenderPassId::RP_BILLBOARD, RenderPassId::RP_BILLBOARD),
    (RenderPassId::RP_STARS, RenderPassId::RP_SKY_GRADIENT),
    (RenderPassId::RP_FOG_GRADIENT, RenderPassId::RP_FOG_GRADIENT),
];

/// Compute the packed stencil-*test* bits for `mask_bit`: `StencilEnable`, `StencilFunc = EQUAL` against a
/// zero reference through the mask bit, all ops `KEEP` (the test never writes). OR this into a draw's
/// packed index (after clearing [`STENCIL_INDEX_MASK`]) with a zero stencil reference so the tagged
/// peripheral pixels -- whose masked bits are non-zero -- fail the `0 == masked` test and are skipped.
pub fn packed_stencil_test(mask_bit: u32) -> u64 {
    const COMPARISON_EQUAL: u64 = 3; // D3D11_COMPARISON_EQUAL
    const STENCIL_OP_KEEP: u64 = 1; // D3D11_STENCIL_OP_KEEP
    let read_mask = u64::from(mask_bit) & 0xFF;
    STENCIL_ENABLE_BIT
        | (COMPARISON_EQUAL << 7)        // StencilFunc (bits 7..10)
        | (read_mask << 11)              // StencilReadMask (bits 11..18)
        // StencilWriteMask (bits 19..26) left 0: the test must not write.
        | (STENCIL_OP_KEEP << 27)        // StencilFailOp (bits 27..30)
        | (STENCIL_OP_KEEP << 32)        // StencilDepthFailOp (high dword bits 0..3)
        | (STENCIL_OP_KEEP << 36) // StencilPassOp (high dword bits 4..7)
}

/// Inject the peripheral stencil test into `ctx`'s staged depth-stencil index, if foveation is forcing it
/// and this draw doesn't already use stencil. Called from the `SetupRenderStates` detour before the
/// original flushes the index to D3D. A no-op unless [`FORCE_STENCIL_TEST`] is set and
/// [`STENCIL_TEST_BITS`] is non-zero, so the common (non-foveated) path costs two relaxed atomic loads.
///
/// # Safety
///
/// `ctx` must be the live render context `SetupRenderStates` was called with.
pub unsafe fn apply_force_test(ctx: *mut HContext_t) {
    if !FORCE_STENCIL_TEST.load(Ordering::Relaxed) {
        return;
    }
    let bits = STENCIL_TEST_BITS.load(Ordering::Relaxed);
    if bits == 0 {
        return;
    }
    let Some(ctx) = (unsafe { ctx.as_mut() }) else {
        return;
    };
    // Draws that already drive stencil (their own masking) are left untouched.
    if ctx.m_DepthStencilStateIndex & STENCIL_ENABLE_BIT != 0 {
        return;
    }
    ctx.m_DepthStencilStateIndex = (ctx.m_DepthStencilStateIndex & !STENCIL_INDEX_MASK) | bits;
    ctx.m_StencilRef = 0;
}

/// Run the mask-write pass for one eye: tag the dropped peripheral pixels in the main depth-stencil
/// surface's stencil with the mask bit. Acquires the engine context internally under its mutex.
pub fn mask_write(params: FoveationParams) -> anyhow::Result<()> {
    with_context(|device, context| {
        let mut guard = FOVEATION.lock();
        if guard.is_none() {
            *guard = Some(Foveation::new(device)?);
        }
        let fov = guard
            .as_mut()
            .expect("the foveation pipeline was just ensured");
        // SAFETY: `context` is the live engine immediate context, held under its mutex by `with_context`.
        unsafe { fov.mask_write(device, context, params) }
    })
}

/// Run the fill-in pass for one eye: reconstruct the dropped peripheral pixels in the main colour buffer
/// from their kept neighbours, drawing every pixel (the kept ones pass through). For a point in the
/// frame where the mask bit may no longer be in the stencil. Acquires the engine context internally
/// under its mutex.
#[inline]
pub fn fill_in(params: FoveationParams) -> anyhow::Result<()> {
    fill_in_with(params, false)
}

/// Run the fill-in pass for one eye under a stencil test against the mask bit, so only the dropped
/// pixels run the pixel shader. The mask bit must still be in the main depth surface's stencil.
#[inline]
pub fn fill_in_dropped(params: FoveationParams) -> anyhow::Result<()> {
    fill_in_with(params, true)
}

fn fill_in_with(params: FoveationParams, stenciled: bool) -> anyhow::Result<()> {
    with_context(|device, context| {
        let mut guard = FOVEATION.lock();
        if guard.is_none() {
            *guard = Some(Foveation::new(device)?);
        }
        let fov = guard
            .as_mut()
            .expect("the foveation pipeline was just ensured");
        // SAFETY: `context` is the live engine immediate context, held under its mutex by `with_context`.
        unsafe { fov.fill_in(device, context, params, stenciled) }
    })
}

/// Tear down the foveation pipeline (COM release). Called from the VR runtime cleanup.
pub fn teardown() {
    *FOVEATION.lock() = None;
    FORCE_STENCIL_TEST.store(false, Ordering::Relaxed);
    STENCIL_TEST_BITS.store(0, Ordering::Relaxed);
}

/// The committed, precompiled foveation shaders. The vertex shader is the shared fullscreen-triangle from
/// the capture composite; the pixel shaders are foveation-specific.
const VERTEX_DXBC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/capture_vs.dxbc"
));
const MASK_DXBC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/foveation_mask_ps.dxbc"
));
const FILL_DXBC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/foveation_fill_ps.dxbc"
));

/// The foveation pipeline singleton, built lazily on first use and torn down with the runtime. Holds COM
/// objects, which `windows` marks `Send`/`Sync`, so a `Mutex` static is sound.
static FOVEATION: Mutex<Option<Foveation>> = Mutex::new(None);

/// Acquire the engine device and immediate context, run `body` under the context mutex, and return its
/// result. Mirrors [`crate::vr::blit`]'s context discipline.
fn with_context<T>(
    body: impl FnOnce(&Device, &ID3D11DeviceContext) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        .context("foveation: the graphics engine is unavailable")?;
    let device =
        unsafe { ge.m_Device.as_ref() }.context("foveation: the graphics device is unavailable")?;
    let context = unsafe { device.m_Context.as_ref() }
        .context("foveation: the graphics context is unavailable")?;
    unsafe {
        EnterCriticalSection(context.m_Mutex);
        let result = body(device, &context.m_Context);
        LeaveCriticalSection(context.m_Mutex);
        result
    }
}

/// The foveation pipeline: shaders, the two full-screen depth-stencil / rasterizer states, the per-pass
/// constant buffer, the fill-in colour copy target, and caches of the views over the engine's main
/// surfaces (keyed by texture pointer so a resize rebuilds them).
struct Foveation {
    vertex_shader: ID3D11VertexShader,
    mask_shader: ID3D11PixelShader,
    fill_shader: ID3D11PixelShader,
    /// Writes the mask bit where the mask shader keeps the fragment (REPLACE on pass, depth off).
    mask_state: ID3D11DepthStencilState,
    /// Passes only the pixels carrying the mask bit, writing nothing (depth off).
    fill_state: ID3D11DepthStencilState,
    rasterizer: ID3D11RasterizerState,
    /// The `FoveationParams` constant buffer (`b0`), updated per pass.
    params_cb: ID3D11Buffer,
    /// The mask bit the current `mask_state` was built for; a config change rebuilds it.
    mask_state_bit: u32,
    /// Depth-stencil view over the main depth surface, keyed by its texture pointer.
    dsv_cache: Option<(usize, ID3D11DepthStencilView)>,
    /// Render-target view over the main colour buffer, keyed by its texture pointer.
    rtv_cache: Option<(usize, ID3D11RenderTargetView)>,
    /// The fill-in colour copy: a scratch texture and its SRV, rebuilt on a size/pointer change.
    copy: Option<FillCopy>,
}

/// The scratch colour copy the fill-in pass reads while writing the main colour buffer.
struct FillCopy {
    texture: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    width: u32,
    height: u32,
    format: i32,
}

impl Foveation {
    fn new(device: &Device) -> anyhow::Result<Self> {
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors are valid for these calls.
        unsafe {
            let mut vertex_shader = None;
            d3d.CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("foveation: creating the vertex shader")?;
            let vertex_shader =
                vertex_shader.context("foveation: the vertex shader was not created")?;

            let mut mask_shader = None;
            d3d.CreatePixelShader(MASK_DXBC, None, Some(&mut mask_shader))
                .context("foveation: creating the mask pixel shader")?;
            let mask_shader =
                mask_shader.context("foveation: the mask pixel shader was not created")?;

            let mut fill_shader = None;
            d3d.CreatePixelShader(FILL_DXBC, None, Some(&mut fill_shader))
                .context("foveation: creating the fill pixel shader")?;
            let fill_shader =
                fill_shader.context("foveation: the fill pixel shader was not created")?;

            let mask_state_bit = 0x80;
            let mask_state = create_mask_state(d3d, mask_state_bit)?;
            let fill_state = create_fill_state(d3d, mask_state_bit)?;

            let mut rasterizer = None;
            d3d.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )
            .context("foveation: creating the rasterizer state")?;
            let rasterizer =
                rasterizer.context("foveation: the rasterizer state was not created")?;

            let params_cb = create_params_cb(d3d)?;

            Ok(Self {
                vertex_shader,
                mask_shader,
                fill_shader,
                mask_state,
                fill_state,
                rasterizer,
                params_cb,
                mask_state_bit,
                dsv_cache: None,
                rtv_cache: None,
                copy: None,
            })
        }
    }

    /// Write the mask bit into the main depth surface's stencil for the dropped peripheral pixels.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context, held under its mutex.
    unsafe fn mask_write(
        &mut self,
        device: &Device,
        context: &ID3D11DeviceContext,
        params: FoveationParams,
    ) -> anyhow::Result<()> {
        if params.mask_bit != self.mask_state_bit {
            self.mask_state = unsafe { create_mask_state(&device.m_Device, params.mask_bit)? };
            self.fill_state = unsafe { create_fill_state(&device.m_Device, params.mask_bit)? };
            self.mask_state_bit = params.mask_bit;
        }
        let (dsv, width, height) = self.main_depth(device)?;
        unsafe {
            let backup = capture_state(context);
            self.update_params(context, params, width, height);
            context.OMSetRenderTargets(None, Some(&dsv));
            context.OMSetDepthStencilState(&self.mask_state, params.mask_bit);
            self.bind_fullscreen(context, &self.mask_shader, width, height);
            context.Draw(3, 0);
            restore_state(context, &backup);
        }
        Ok(())
    }

    /// Reconstruct the dropped peripheral pixels in the main colour buffer from their kept neighbours.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context, held under its mutex.
    unsafe fn fill_in(
        &mut self,
        device: &Device,
        context: &ID3D11DeviceContext,
        params: FoveationParams,
        stenciled: bool,
    ) -> anyhow::Result<()> {
        if params.mask_bit != self.mask_state_bit {
            self.mask_state = unsafe { create_mask_state(&device.m_Device, params.mask_bit)? };
            self.fill_state = unsafe { create_fill_state(&device.m_Device, params.mask_bit)? };
            self.mask_state_bit = params.mask_bit;
        }
        let dsv = if stenciled {
            Some(self.main_depth(device)?.0)
        } else {
            None
        };
        let (rtv, color_tex, width, height, format) = self.main_color(device)?;
        let srv = self.ensure_copy(device, &color_tex, width, height, format)?;
        unsafe {
            // Snapshot the shaded colour so the fill can read neighbours while writing the same buffer.
            let copy_tex = self
                .copy
                .as_ref()
                .expect("the fill copy was just ensured")
                .texture
                .clone();
            context.CopyResource(&copy_tex, &color_tex);
            let backup = capture_state(context);
            self.update_params(context, params, width, height);
            context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), dsv.as_ref());
            if dsv.is_some() {
                context.OMSetDepthStencilState(&self.fill_state, params.mask_bit);
            } else {
                context.OMSetDepthStencilState(None, 0);
            }
            self.bind_fullscreen(context, &self.fill_shader, width, height);
            context.PSSetShaderResources(0, Some(&[Some(srv.clone())]));
            context.Draw(3, 0);
            context.PSSetShaderResources(0, Some(&[None]));
            restore_state(context, &backup);
        }
        Ok(())
    }

    /// Bind the shared full-screen pipeline state (input assembler, shaders, rasterizer, viewport, params
    /// buffer) for a foveation pass. The output-merger targets and any SRVs are set by the caller.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context, held under its mutex.
    unsafe fn bind_fullscreen(
        &self,
        context: &ID3D11DeviceContext,
        pixel_shader: &ID3D11PixelShader,
        width: u32,
        height: u32,
    ) {
        unsafe {
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: width as f32,
                Height: height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(pixel_shader, None);
            context.PSSetConstantBuffers(0, Some(&[Some(self.params_cb.clone())]));
            context.RSSetState(&self.rasterizer);
            // Opaque blend, explicitly: this raw pass otherwise inherits whatever blend state the
            // engine's last block left on the context. An inherited additive/alpha state makes the
            // fill *add* its full-screen reconstruction on top of the already-shaded scene instead
            // of replacing the dropped pixels -- a whole-frame ~2x brightness step that latches for
            // as long as the scene keeps handing this pass the same leftover state (the
            // pose-latched global brightness flips).
            context.OMSetBlendState(None, None, 0xFFFF_FFFF);
        }
    }

    /// Update the params constant buffer for this pass.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context, held under its mutex.
    unsafe fn update_params(
        &self,
        context: &ID3D11DeviceContext,
        params: FoveationParams,
        width: u32,
        height: u32,
    ) {
        let data = pass_constants(&params, width, height);
        unsafe {
            context.UpdateSubresource(&self.params_cb, 0, None, data.as_ptr().cast(), 0, 0);
        }
    }

    /// Get (creating and caching, rebuilding on a texture change) the depth-stencil view over the engine's
    /// main depth surface, with its pixel dimensions.
    fn main_depth(
        &mut self,
        device: &Device,
    ) -> anyhow::Result<(ID3D11DepthStencilView, u32, u32)> {
        let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
            .context("foveation: the graphics engine is unavailable")?;
        let tex = unsafe { ge.m_MainDepthTexture.as_ref() }
            .context("foveation: the main depth texture is unavailable")?;
        let ptr = ge.m_MainDepthTexture as usize;
        let dsv = view_ptr::<ID3D11DepthStencilView>(addr_of!(tex.m_DSV))
            .context("foveation: the main depth texture has no depth-stencil view")?;
        if self.dsv_cache.as_ref().map(|(p, _)| *p) != Some(ptr) {
            self.dsv_cache = Some((ptr, dsv.clone()));
        }
        let _ = device;
        Ok((dsv, u32::from(tex.m_Width), u32::from(tex.m_Height)))
    }

    /// Get (creating and caching, rebuilding on a texture change) the render-target view over the engine's
    /// main colour buffer, the colour texture itself, its dimensions, and its `DXGI_FORMAT`.
    fn main_color(
        &mut self,
        device: &Device,
    ) -> anyhow::Result<(ID3D11RenderTargetView, ID3D11Texture2D, u32, u32, i32)> {
        let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
            .context("foveation: the graphics engine is unavailable")?;
        let tex = unsafe { ge.m_MainColorBuffer.as_ref() }
            .context("foveation: the main colour buffer is unavailable")?;
        let ptr = ge.m_MainColorBuffer as usize;
        let rtv = view_ptr::<ID3D11RenderTargetView>(addr_of!(tex.m_RTV))
            .context("foveation: the main colour buffer has no render-target view")?;
        let color_tex = resource_as_texture(addr_of!(tex.m_Texture))
            .context("foveation: the main colour buffer has no backing texture")?;
        if self.rtv_cache.as_ref().map(|(p, _)| *p) != Some(ptr) {
            self.rtv_cache = Some((ptr, rtv.clone()));
        }
        let _ = device;
        Ok((
            rtv,
            color_tex,
            u32::from(tex.m_Width),
            u32::from(tex.m_Height),
            tex.m_Format as i32,
        ))
    }

    /// Ensure the fill-in colour copy matches the main colour buffer's size/format, (re)creating it if
    /// not, and return its SRV.
    fn ensure_copy(
        &mut self,
        device: &Device,
        color_tex: &ID3D11Texture2D,
        width: u32,
        height: u32,
        format: i32,
    ) -> anyhow::Result<ID3D11ShaderResourceView> {
        if let Some(copy) = &self.copy
            && copy.width == width
            && copy.height == height
            && copy.format == format
        {
            return Ok(copy.srv.clone());
        }
        let _ = color_tex;
        let d3d = &device.m_Device;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT(format),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture = None;
        unsafe { d3d.CreateTexture2D(&desc, None, Some(&mut texture)) }
            .context("foveation: creating the fill-in colour copy texture")?;
        let texture =
            texture.context("foveation: the fill-in colour copy texture was not created")?;
        let mut srv = None;
        unsafe { d3d.CreateShaderResourceView(&texture, None, Some(&mut srv)) }
            .context("foveation: creating the fill-in colour copy view")?;
        let srv = srv.context("foveation: the fill-in colour copy view was not created")?;
        self.copy = Some(FillCopy {
            texture,
            srv: srv.clone(),
            width,
            height,
            format,
        });
        Ok(srv)
    }
}

/// Build the mask-write depth-stencil state: depth off, stencil enabled, `ALWAYS` pass, `REPLACE` on pass
/// through `mask_bit`, so the fragments the mask shader keeps receive the mask bit and the rest (which it
/// `discard`s) keep their cleared stencil.
///
/// # Safety
/// `d3d` must be the live engine device.
unsafe fn create_mask_state(
    d3d: &ID3D11Device,
    mask_bit: u32,
) -> anyhow::Result<ID3D11DepthStencilState> {
    let op = D3D11_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D11_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
        StencilPassOp: D3D11_STENCIL_OP_REPLACE,
        StencilFunc: D3D11_COMPARISON_ALWAYS,
    };
    let desc = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: false.into(),
        DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
        DepthFunc: D3D11_COMPARISON_ALWAYS,
        StencilEnable: true.into(),
        StencilReadMask: 0,
        StencilWriteMask: (mask_bit & 0xFF) as u8,
        FrontFace: op,
        BackFace: op,
    };
    let mut state = None;
    unsafe { d3d.CreateDepthStencilState(&desc, Some(&mut state)) }
        .context("foveation: creating the mask-write depth-stencil state")?;
    state.context("foveation: the mask-write depth-stencil state was not created")
}

/// Create the fill-in's depth-stencil state: the stencil test passes only where the mask bit is set
/// (`EQUAL` against a reference of the mask bit through the mask bit), and nothing is written.
///
/// # Safety
/// `d3d` must be the live engine device.
unsafe fn create_fill_state(
    d3d: &ID3D11Device,
    mask_bit: u32,
) -> anyhow::Result<ID3D11DepthStencilState> {
    let op = D3D11_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D11_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
        StencilPassOp: D3D11_STENCIL_OP_KEEP,
        StencilFunc: D3D11_COMPARISON_EQUAL,
    };
    let desc = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: false.into(),
        DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
        DepthFunc: D3D11_COMPARISON_ALWAYS,
        StencilEnable: true.into(),
        StencilReadMask: (mask_bit & 0xFF) as u8,
        StencilWriteMask: 0,
        FrontFace: op,
        BackFace: op,
    };
    let mut state = None;
    unsafe { d3d.CreateDepthStencilState(&desc, Some(&mut state)) }
        .context("foveation: creating the fill-in depth-stencil state")?;
    state.context("foveation: the fill-in depth-stencil state was not created")
}

/// Create the 32-byte `FoveationParams` constant buffer (`b0`).
///
/// # Safety
/// `d3d` must be the live engine device.
unsafe fn create_params_cb(d3d: &ID3D11Device) -> anyhow::Result<ID3D11Buffer> {
    let mut buffer = None;
    unsafe {
        d3d.CreateBuffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: std::mem::size_of::<[f32; 12]>() as u32,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
                StructureByteStride: 0,
            },
            None,
            Some(&mut buffer),
        )
    }
    .context("foveation: creating the params constant buffer")?;
    buffer.context("foveation: the params constant buffer was not created")
}

/// The engine pipeline state a foveation pass overwrites, captured so it can be restored afterwards. The
/// engine's render loop binds render targets, the depth-stencil state, the viewport, and the rasterizer
/// once per pass (not per draw) and assumes they persist; a foveation pass that changed them without
/// restoring would leave the following passes rendering into the wrong (or no) target -- whole passes
/// vanish. The per-draw state (shaders, input layout, constant buffers, shader resources) is re-bound by
/// every engine draw, so it is not captured.
pub struct StateBackup {
    blend_state: Option<ID3D11BlendState>,
    blend_factor: [f32; 4],
    sample_mask: u32,
    render_targets: [Option<ID3D11RenderTargetView>; 8],
    depth_stencil_view: Option<ID3D11DepthStencilView>,
    depth_stencil_state: Option<ID3D11DepthStencilState>,
    stencil_ref: u32,
    viewports: Vec<D3D11_VIEWPORT>,
    rasterizer: Option<ID3D11RasterizerState>,
}

/// Capture the pipeline state a foveation pass is about to overwrite.
///
/// # Safety
/// `context` must be the live engine immediate context, held under its mutex.
pub unsafe fn capture_state(context: &ID3D11DeviceContext) -> StateBackup {
    let mut render_targets: [Option<ID3D11RenderTargetView>; 8] = std::array::from_fn(|_| None);
    let mut depth_stencil_view: Option<ID3D11DepthStencilView> = None;
    let mut depth_stencil_state: Option<ID3D11DepthStencilState> = None;
    let mut stencil_ref: u32 = 0;
    let mut num_viewports: u32 = 0;
    let mut blend_state: Option<ID3D11BlendState> = None;
    let mut blend_factor = [0.0f32; 4];
    let mut sample_mask: u32 = 0xFFFF_FFFF;
    unsafe {
        context.OMGetRenderTargets(Some(&mut render_targets), Some(&mut depth_stencil_view));
        context.OMGetDepthStencilState(Some(&mut depth_stencil_state), Some(&mut stencil_ref));
        context.OMGetBlendState(
            Some(&mut blend_state),
            Some(&mut blend_factor),
            Some(&mut sample_mask),
        );
        context.RSGetViewports(&mut num_viewports, None);
    }
    let mut viewports = vec![D3D11_VIEWPORT::default(); num_viewports as usize];
    if num_viewports > 0 {
        unsafe { context.RSGetViewports(&mut num_viewports, Some(viewports.as_mut_ptr())) };
    }
    let rasterizer = unsafe { context.RSGetState() }.ok();
    StateBackup {
        blend_state,
        blend_factor,
        sample_mask,
        render_targets,
        depth_stencil_view,
        depth_stencil_state,
        stencil_ref,
        viewports,
        rasterizer,
    }
}

/// Restore the pipeline state captured by [`capture_state`].
///
/// # Safety
/// `context` must be the live engine immediate context, held under its mutex.
pub unsafe fn restore_state(context: &ID3D11DeviceContext, backup: &StateBackup) {
    unsafe {
        context.OMSetRenderTargets(
            Some(&backup.render_targets),
            backup.depth_stencil_view.as_ref(),
        );
        context.OMSetDepthStencilState(backup.depth_stencil_state.as_ref(), backup.stencil_ref);
        context.OMSetBlendState(
            backup.blend_state.as_ref(),
            Some(&backup.blend_factor),
            backup.sample_mask,
        );
        if !backup.viewports.is_empty() {
            context.RSSetViewports(Some(&backup.viewports));
        }
        context.RSSetState(backup.rasterizer.as_ref());
    }
}

/// Borrow a COM view stored inline in an engine `Texture` (its `m_SRV`/`m_RTV`/`m_DSV` fields) as an owned
/// (AddRef'd) handle, returning `None` if the slot is null. The field is read as a raw pointer so a null
/// slot never materializes a non-null-invariant `windows` interface.
pub fn view_ptr<T: windows::core::Interface>(field: *const T) -> Option<T> {
    // SAFETY: `field` addresses a live `Texture`'s inline COM slot; reading it as a raw pointer and
    // borrowing (no AddRef) is sound, and `.map(Clone::clone)` AddRefs only a non-null handle.
    let raw = unsafe { *(field as *const *mut c_void) };
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}

/// Borrow the engine `Texture`'s inline `ID3D11Resource` (`m_Texture`) and query it to `ID3D11Texture2D`,
/// returning `None` if the slot is null or the cast fails.
pub fn resource_as_texture(
    field: *const jc3gi::graphics_engine::texture::ID3D11Resource,
) -> Option<ID3D11Texture2D> {
    let raw = unsafe { *(field as *const *mut c_void) };
    let resource =
        unsafe { windows::Win32::Graphics::Direct3D11::ID3D11Resource::from_raw_borrowed(&raw) }?;
    resource.cast::<ID3D11Texture2D>().ok()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn params(layout: EyeLayout) -> FoveationParams {
        FoveationParams {
            layout,
            inner_fraction: 0.5,
            outer_fraction: 1.0,
            max_drop: 0.8,
            mask_bit: 0x80,
            debug_show_mask: false,
            cell_px: 2,
        }
    }

    #[test]
    fn the_gbuffer_switch_extends_the_foveated_passes() {
        let base = FoveatedPasses {
            gbuffer: false,
            lighting: false,
        };
        let gbuffer = FoveatedPasses {
            gbuffer: true,
            lighting: false,
        };
        for pass in [
            RenderPassId::RP_TERRAIN_APPLY_NEAR,
            RenderPassId::RP_ROAD_JUNCTION_OPAQUE,
            RenderPassId::RP_DECALS,
        ] {
            assert!(!base.contains(pass as i32));
            assert!(gbuffer.contains(pass as i32));
        }
        for pass in [
            RenderPassId::RP_Z_AND_VELOCITY_PASS,
            RenderPassId::RP_ROAD_STENCIL,
            RenderPassId::RP_TERRAINPATCH_DETAIL_MID,
            RenderPassId::RP_TERRAINPATCH_BASEMESH_NEAR,
            RenderPassId::RP_DOWNSAMPLE_DEPTH,
            RenderPassId::RP_SSAO,
            RenderPassId::RP_DEFERRED_LIGHTS,
        ] {
            assert!(!gbuffer.contains(pass as i32));
        }
        assert!(base.contains(RenderPassId::RP_VEGETATION_OPAQUE as i32));
        assert!(gbuffer.first() < base.first());
        assert!(gbuffer.contains(gbuffer.first()));
        assert_eq!(gbuffer.fill_before(), None);
    }

    #[test]
    fn the_lighting_switch_foveates_the_lighting_and_fills_after_it() {
        let lighting = FoveatedPasses {
            gbuffer: false,
            lighting: true,
        };
        for pass in [
            RenderPassId::RP_DEFERRED_LIGHTS,
            RenderPassId::RP_SKYBOX,
            RenderPassId::RP_FOG_GRADIENT,
        ] {
            assert!(lighting.contains(pass as i32));
        }
        for pass in [
            RenderPassId::RP_SSAO,
            RenderPassId::RP_SCREEN_SPACE_REFLECTIONS,
            RenderPassId::RP_OCCLUSION_QUERY,
            RenderPassId::RP_COPY_FRAMEBUFFER,
            RenderPassId::RP_WATER,
        ] {
            assert!(!lighting.contains(pass as i32));
        }
        let fill = lighting.fill_before().expect("the fill-in moves");
        assert!(lighting.contains(fill - 1));
        assert!(fill <= RenderPassId::RP_COPY_FRAMEBUFFER as i32);
        assert!((fill..RenderPassId::RP_RENDERPASS_COUNT as i32).all(|p| !lighting.contains(p)));
    }

    #[test]
    fn one_eye_uses_the_whole_target_and_never_splits() {
        let constants = pass_constants(
            &params(EyeLayout::Single {
                center_uv: glam::Vec2::new(0.25, 0.5),
            }),
            300,
            400,
        );
        // Half-diagonal of 300 x 400 is 250.
        assert_eq!(
            constants,
            [
                75.0, 200.0, 75.0, 200.0, 125.0, 250.0, 0.8, 0.0, 301.0, 2.0, 0.0, 0.0
            ]
        );
    }

    #[test]
    fn side_by_side_eyes_get_eye_sized_radii_and_eye_local_centres() {
        let constants = pass_constants(
            &params(EyeLayout::SideBySide {
                center_uv: [glam::Vec2::new(0.6, 0.5), glam::Vec2::new(0.4, 0.25)],
            }),
            600,
            400,
        );
        // Each eye is 300 x 400, whose half-diagonal is 250, and eye 1 starts at column 300.
        assert_eq!(
            constants,
            [
                180.0, 200.0, 120.0, 100.0, 125.0, 250.0, 0.8, 0.0, 300.0, 2.0, 0.0, 0.0
            ]
        );
    }
}
