//! The desktop mirror: while an OpenXR session runs, show one eye, or both side by side, in the
//! game's own window.
//!
//! ## Why a mirror is needed
//!
//! While a session runs the compositor owns the HMD present and the engine's own present is
//! suppressed for both eyes (`BLOCK_FLIP`, `docs/engine/rendering/rendering.md` §7), so the game window would freeze
//! on a stale frame. This module presents the game's own swapchain itself -- exactly once per frame,
//! unsynced -- with the configured [`MirrorView`]'s eye captures drawn into the back buffer.
//!
//! ## Framing and the stretch pre-compensation
//!
//! With `vr.own_back_buffer` on -- the default -- the swapchain stays at the window size
//! ([`crate::vr::back_buffer`]), so the present does not stretch and the buffer-space viewport *is*
//! the window-space one.
//!
//! The pre-compensation below exists for the paths where it is not: with `vr.own_back_buffer` off,
//! the engine's swapchain buffers are resized to the per-eye render resolution
//! (`vr.native_resolution`, `docs/engine/rendering/rendering.md` §9), which is near-square, while the Win32 window
//! keeps its original (usually 16:9) client rect. DXGI's BitBlt present then stretches the whole back
//! buffer onto the window client rect, so a straight full-buffer draw would be distorted.
//! [`mirror_viewport`] computes a viewport *in buffer space* such that, after the buffer→window
//! stretch, the eye image lands at its own aspect, centered; with buffer == window that computation
//! degenerates to the identity on its own (unit-tested), so ownership needs no special case here.
//! Once ownership is the only mode the compensation can be deleted outright. A window resize by the
//! WM only changes the client rect, so the viewport recomputes next frame; the VR path is never
//! touched.
//!
//! Within that, [`MirrorFraming`] picks which way the two aspects are reconciled. `Fill` (what other
//! VR titles' desktop views do) scales the image up until it covers the window and lets the overflow
//! crop; the resulting viewport deliberately extends past the buffer edges, and the rasterizer clips
//! it against the render target -- so the crop costs nothing beyond a little overdraw, with no
//! scissor, extra copy, or UV-transform shader. `Fit` scales down and letterboxes instead, showing the
//! whole image. A zoom magnifies further about the centre in either mode. [`mirror_layout`] fills a
//! single eye and fits the stereo pair, and the egui overlay composite always uses `Fit` at zoom
//! `1.0`: a cropped pair would not fuse, and a cropped UI panel would be unreadable.
//!
//! ## Gamma
//!
//! Straight passthrough, no gamma conversion (unlike [`crate::vr::blit`], which linearizes for an
//! `_SRGB` swapchain). The captured eye texture is a `CopyResource` of `m_BackBufferLinear` as
//! `R8G8B8A8_UNORM` (non-sRGB) holding **display-referred** bytes -- the same bytes the game itself
//! presents to its non-sRGB desktop swapchain, which look correct. The game swapchain back buffer we
//! draw into is that same non-sRGB format, so an RTV over it applies **no** hardware encode: writing
//! the sampled bytes unchanged reproduces exactly what the game would have presented. So the mirror
//! reuses the capture composite's plain-sample pixel shader; no gamma is a genuine correctness
//! conclusion, not a guess, so it is not a config knob.
//!
//! ## Threading
//!
//! Everything runs on the game's main thread, after both eyes have drawn and the draw worker has
//! drained (`hooks::game`). The letterbox draw, egui composite, and present run under the engine's
//! `Context::m_Mutex`, serialized with the engine's render work, the same discipline as
//! [`crate::vr::blit`].

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::Context as _;
use jc3gi::graphics_engine::device::Device;
use parking_lot::Mutex;
use windows::{
    Win32::{
        Graphics::{
            Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            Direct3D11::{
                D3D11_BLEND_DESC, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD,
                D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_COMPARISON_NEVER, D3D11_CULL_NONE,
                D3D11_FILL_SOLID, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_RASTERIZER_DESC,
                D3D11_RENDER_TARGET_BLEND_DESC, D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_CLAMP,
                D3D11_TEXTURE2D_DESC, D3D11_VIEWPORT, ID3D11BlendState, ID3D11DeviceContext,
                ID3D11PixelShader, ID3D11RasterizerState, ID3D11RenderTargetView,
                ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
            },
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface as _,
};

use crate::{
    config::{self, Config, Switch, disable_automatically},
    recorder::{MirrorDraw, MirrorFrame},
    ui::render::{EGUI_DEBUG_RENDER_STATE, eye_capture_fresh},
    vr::{
        MirrorView,
        mirror::layout::{
            AspectSize, MirrorFraming, MirrorLayout, Viewport, mirror_layout, mirror_viewport,
        },
    },
};

mod layout;

/// The committed, precompiled shaders shared with the VR blit and others: the fullscreen-triangle
/// vertex shader and the plain-sample (no gamma) pixel shader. See the module gamma note.
const VERTEX_DXBC: &[u8] = include_bytes!("../../shaders/capture_vs.dxbc");
const PIXEL_DXBC: &[u8] = include_bytes!("../../shaders/capture_ps.dxbc");

/// The mirror pipeline singleton, built lazily on the first mirrored frame and torn down with the
/// runtime. Holds COM objects, which `windows` marks `Send`/`Sync`, so a `Mutex` static is sound.
static MIRROR: Mutex<Option<Mirror>> = Mutex::new(None);

/// Whether [`present_mirror`] is currently in a transient-failure streak, so repeated failures log a
/// transition and a recovery instead of one line per frame. Mirrors the pattern in
/// [`crate::vr::frame_begin`]'s `FRAME_BEGIN_FAILING`.
static MIRROR_FAILING: AtomicBool = AtomicBool::new(false);
/// Frames lost to the current (or just-ended) transient mirror-present failure streak. Paired with
/// [`MIRROR_FAILING`].
static MIRROR_FAIL_COUNT: AtomicU32 = AtomicU32::new(0);

/// Draw `view`'s eye captures into the game swapchain's back buffer (laid out per
/// [`mirror_layout`]), composite the egui overlay on top, and present the game swapchain unsynced.
///
/// Called once per frame from the stereo Draw driver, after the eyes have drawn and drained and the
/// XR frame has been submitted, only while a session is running and `vr.mirror` is on. Any failure
/// logs on target `"vr"`, disables `vr.mirror` at runtime (the game window then holds its last frame),
/// and never crashes or wedges the frame loop.
///
/// **Unsynced by mandate:** the present uses `SyncInterval = 0`. A vsynced mirror on a 60 Hz monitor
/// would block the game thread until the monitor's next scanout and throttle the whole frame loop --
/// including the 90 Hz HMD submit -- down to the monitor's refresh. The compositor, not this present,
/// paces the HMD.
pub fn present_mirror(view: MirrorView) {
    if let Err(e) = unsafe { present_mirror_inner(view) } {
        // Device-removed errors are fatal: the D3D11 device is gone, so the mirror cannot recover.
        // Other errors (e.g. a transient occlusion, surface loss) may clear on the next frame, so
        // rate-limit the warning (transition + periodic reminder) rather than flooding the log.
        if let Some(win_err) = e
            .chain()
            .filter_map(|src| src.downcast_ref::<windows::core::Error>())
            .find(|win_err| {
                let code = win_err.code();
                code == windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED
                    || code == windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_HUNG
                    || code == windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_RESET
            })
        {
            tracing::error!(target: "vr", "mirror present failed with device-removed error {win_err}; disabling vr.mirror");
            disable_automatically(Switch {
                path: "vr.mirror",
                place: |c| &mut c.vr.mirror,
            });
            *MIRROR.lock() = None;
        } else {
            let count = MIRROR_FAIL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if !MIRROR_FAILING.swap(true, Ordering::Relaxed) {
                tracing::warn!(target: "vr", "mirror present failed (transient); will retry: {e:#}");
            } else if count.is_multiple_of(90) {
                tracing::warn!(target: "vr", count, "mirror present still failing: {e:#}");
            }
        }
    } else {
        if MIRROR_FAILING.swap(false, Ordering::Relaxed) {
            let frames = MIRROR_FAIL_COUNT.swap(0, Ordering::Relaxed);
            tracing::info!(target: "vr", frames, "mirror present recovered");
        }
    }
}

/// Tear down the mirror pipeline (COM release), dropping the cached views over swapchain buffer 0
/// with it; the next mirrored frame rebuilds them lazily. Called from the runtime cleanup, and from
/// [`crate::vr::back_buffer::sync_swapchain_to_window`] before it resizes the swapchain, since
/// `IDXGISwapChain::ResizeBuffers` fails while any reference to buffer 0 outstands.
pub fn teardown() {
    *MIRROR.lock() = None;
}

/// Toggle `vr.mirror_view` between the stereo pair and the single eye it was last switched away
/// from (the left eye before any toggle), returning the new view. Bound to F10.
pub fn toggle_stereo_pair() -> MirrorView {
    let mut last_mono = LAST_MONO_VIEW.lock();
    let mut cfg = config::edit();
    let (view, remembered) = cfg.vr.mirror_view.toggled(*last_mono);
    cfg.vr.mirror_view = view;
    *last_mono = remembered;
    view
}

/// The single eye [`toggle_stereo_pair`] returns to from the stereo pair.
static LAST_MONO_VIEW: Mutex<MirrorView> = Mutex::new(MirrorView::Left);

/// # Safety
/// Reads engine singletons via raw pointer dereference; the graphics engine, device, and context must
/// be live (they are while the hooks are installed and a frame is in flight on the game thread).
unsafe fn present_mirror_inner(view: MirrorView) -> anyhow::Result<()> {
    // An eye went straight to the headset without refreshing its capture (the view changed after
    // its dispatch decided): leave the window on the previous frame rather than show a stale
    // capture.
    if !(0..2).all(|eye| !view.shows_eye(eye) || eye_capture_fresh(eye)) {
        return Ok(());
    }

    // Clone (AddRef) the captured eye textures under a brief EGUI lock, dropped before the mirror
    // state / context work, mirroring the blit's lock discipline. Both eye captures share one size.
    let eye_textures: [Option<ID3D11Texture2D>; 2] = {
        let lock = EGUI_DEBUG_RENDER_STATE.lock();
        [0, 1].map(|eye| view.shows_eye(eye).then(|| lock.texture(eye).cloned())?)
    };
    let size_texture = eye_textures
        .iter()
        .flatten()
        .next()
        .context("vr: the mirror eye capture is unavailable")?;

    let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        .context("vr: the graphics engine is unavailable for the mirror")?;
    let device = unsafe { ge.m_Device.as_ref() }
        .context("vr: the graphics device is unavailable for the mirror")?;
    let context = unsafe { device.m_Context.as_ref() }
        .context("vr: the graphics context is unavailable for the mirror")?;
    let back_buffer = unsafe { device.m_BackBuffer.as_ref() }
        .context("vr: the game back buffer is unavailable for the mirror")?;

    let buffer_size = (
        u32::from(back_buffer.m_Width),
        u32::from(back_buffer.m_Height),
    );
    let (src_w, src_h) = unsafe { texture_size(size_texture) };
    let window_size = crate::vr::window::client_size()
        .context("vr: the game window client rect is unavailable")?;
    let layout = mirror_layout(
        view,
        AspectSize {
            width: src_w,
            height: src_h,
        },
        buffer_size,
        window_size,
    );

    // The engine wraps swapchain back buffer 0 (`docs/engine/rendering/rendering.md` §7); drawing into its resource and
    // then presenting the swapchain is the engine's own present path, minus the vsync/bookkeeping.
    let back_texture = &back_buffer.m_Texture;

    let mut guard = MIRROR.lock();
    if guard.is_none() {
        *guard = Some(Mirror::new(device)?);
    }
    let mirror = guard
        .as_mut()
        .expect("the mirror pipeline was just ensured");

    let rtv = mirror.rtv_for(device, back_texture)?;
    let mut srvs: [Option<ID3D11ShaderResourceView>; 2] = [None, None];
    for (eye, texture) in eye_textures.iter().enumerate() {
        if let Some(texture) = texture {
            srvs[eye] = Some(mirror.srv_for(device, eye, texture)?);
        }
    }

    // The eye draws, egui composite, and present run under one critical-section hold, serialized
    // with the engine's render work. The section is recursive, so a re-entrant take inside egui (there
    // is none today) would be safe.
    unsafe {
        EnterCriticalSection(context.m_Mutex);
        mirror.clear(&context.m_Context, &rtv);
        for draw in layout.draws() {
            // A view's eye whose capture is missing (the pair is being rebuilt across a resize)
            // leaves its part of the window black for the frame.
            if let Some(srv) = &srvs[draw.eye] {
                mirror.draw(&context.m_Context, &rtv, srv, draw.viewport);
            }
        }

        // Bring the debug UI to the desktop while a session runs -- the flat overlay normally rides
        // the engine flip path, which `BLOCK_FLIP` suppresses in VR. The clean view hides it: the
        // flat overlay at either scope, the floating panel's copy at the everything scope.
        //
        // The egui frame is tessellated once per frame on eye 0 into an offscreen texture -- the
        // panel texture with the floating panel up (issue #24), else the flat mirror-overlay texture
        // (`crate::hud::mirror_overlay`) -- so here we only composite that texture as an
        // alpha-blended overlay. Never `EguiState::render` (a second egui pass that mutates shared
        // state), so this is safe from the deferred frame tail's thread as well as inline.
        if !crate::director::clean_view::hides_everything() {
            let panel = Config::lock_query(|c| c.hud.egui_panel);
            if panel.enabled {
                // With `show_on_mirror` off, the panel stays in the headset only, leaving the desktop
                // mirror showing the unobstructed scene.
                if panel.show_on_mirror
                    && let Some(srv) = crate::hud::egui_panel::panel_srv()
                {
                    // Always letterboxed at 1:1 -- a cropped or magnified UI panel would lose its
                    // edges, whatever the eye image's framing.
                    let overlay = mirror_viewport(
                        AspectSize {
                            width: panel.resolution.0,
                            height: panel.resolution.1,
                        },
                        buffer_size,
                        window_size,
                        MirrorFraming::Fit,
                        1.0,
                    );
                    mirror.draw_overlay(&context.m_Context, &rtv, &srv, overlay);
                }
            } else if !crate::director::clean_view::hides_desktop()
                && let Some(srv) = crate::hud::mirror_overlay::overlay_srv()
            {
                // The flat overlay texture is window-sized, so stretching it across the whole buffer
                // is exactly inverted by the present's buffer→window stretch: it lands on the window
                // at 1:1, undistorted, whatever the render resolution is.
                let full = Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: buffer_size.0 as f32,
                    height: buffer_size.1 as f32,
                };
                mirror.draw_overlay(&context.m_Context, &rtv, &srv, full);
            }
        }

        // Present the game swapchain ourselves -- the only present this frame (the engine's is
        // blocked). Unsynced by mandate (see the doc comment); errors bubble up to disable the mirror.
        let present = device
            .m_SwapChain
            .Present(0, windows::Win32::Graphics::Dxgi::DXGI_PRESENT::default());
        LeaveCriticalSection(context.m_Mutex);
        present
            .ok()
            .context("vr: the mirror swapchain present failed")?;
    }
    crate::recorder::note_mirror(recorded_layout(view, &layout, buffer_size, window_size));
    Ok(())
}

/// The layout as the recorder's sidecar records it: each eye's viewport carried from buffer pixels
/// to window pixels through the present's buffer→window stretch.
fn recorded_layout(
    view: MirrorView,
    layout: &MirrorLayout,
    buffer: (u32, u32),
    window: (u32, u32),
) -> MirrorFrame {
    let sx = window.0 as f32 / buffer.0.max(1) as f32;
    let sy = window.1 as f32 / buffer.1.max(1) as f32;
    let mut draws = [None; 2];
    for (slot, draw) in draws.iter_mut().zip(layout.draws()) {
        let v = draw.viewport;
        *slot = Some(MirrorDraw {
            eye: draw.eye,
            rect: [v.x * sx, v.y * sy, v.width * sx, v.height * sy],
        });
    }
    MirrorFrame {
        view,
        window,
        draws,
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

/// The mirror pipeline: shaders, sampler, rasterizer, and caches of the back-buffer render-target view
/// and the eye-capture shader-resource view, both keyed on the underlying resource pointer so an
/// engine resize (which changes the back-buffer / capture identity) rebuilds the view.
struct Mirror {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    rasterizer: ID3D11RasterizerState,
    /// Premultiplied-alpha blend for the egui panel overlay composite (issue #24). The letterboxed eye
    /// draw is opaque and uses no blend state; only the overlay needs one.
    overlay_blend: ID3D11BlendState,
    /// The RTV over the game back buffer, keyed on the back-buffer texture pointer.
    rtv_cache: Option<(usize, ID3D11RenderTargetView)>,
    /// Per eye, the SRV over its captured texture, keyed on the capture texture pointer.
    srv_cache: [Option<(usize, ID3D11ShaderResourceView)>; 2],
}

impl Mirror {
    fn new(device: &Device) -> anyhow::Result<Self> {
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors are valid for these calls.
        unsafe {
            let mut vertex_shader: Option<ID3D11VertexShader> = None;
            d3d.CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("vr: creating the mirror vertex shader")?;
            let vertex_shader =
                vertex_shader.context("vr: the mirror vertex shader was not created")?;

            let mut pixel_shader: Option<ID3D11PixelShader> = None;
            d3d.CreatePixelShader(PIXEL_DXBC, None, Some(&mut pixel_shader))
                .context("vr: creating the mirror pixel shader")?;
            let pixel_shader =
                pixel_shader.context("vr: the mirror pixel shader was not created")?;

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
            .context("vr: creating the mirror sampler")?;
            let sampler = sampler.context("vr: the mirror sampler was not created")?;

            let mut rasterizer: Option<ID3D11RasterizerState> = None;
            d3d.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )
            .context("vr: creating the mirror rasterizer state")?;
            let rasterizer =
                rasterizer.context("vr: the mirror rasterizer state was not created")?;

            // Premultiplied alpha, matching the HUD quad: the egui panel texture holds
            // premultiplied colour (egui renders premultiplied), so its transparent regions show
            // the eye image behind it and its partially covered pixels are not darkened twice.
            let mut overlay_blend: Option<ID3D11BlendState> = None;
            let mut blend_desc = D3D11_BLEND_DESC::default();
            blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_ONE,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            d3d.CreateBlendState(&blend_desc, Some(&mut overlay_blend))
                .context("vr: creating the mirror overlay blend state")?;
            let overlay_blend =
                overlay_blend.context("vr: the mirror overlay blend state was not created")?;

            Ok(Self {
                vertex_shader,
                pixel_shader,
                sampler,
                rasterizer,
                overlay_blend,
                rtv_cache: None,
                srv_cache: [None, None],
            })
        }
    }

    /// Clear the whole back buffer to opaque black: the letterbox bars around the stereo pair. Under
    /// a single eye's fill the draw covers the whole target and the clear is redundant, but it costs
    /// nothing next to the present. The caller must hold the context mutex.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context; `rtv` must be a live view over the game
    /// back buffer.
    unsafe fn clear(&self, context: &ID3D11DeviceContext, rtv: &ID3D11RenderTargetView) {
        unsafe {
            context.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0]);
        }
    }

    /// Draw one eye into `viewport`, opaque. The caller must hold the context mutex.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context; `rtv`/`srv` must be live views over the
    /// game back buffer and the captured eye texture respectively.
    unsafe fn draw(
        &self,
        context: &ID3D11DeviceContext,
        rtv: &ID3D11RenderTargetView,
        srv: &ID3D11ShaderResourceView,
        viewport: Viewport,
    ) {
        unsafe {
            context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: viewport.x,
                TopLeftY: viewport.y,
                Width: viewport.width,
                Height: viewport.height,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.PSSetShaderResources(0, Some(&[Some(srv.clone())]));
            context.RSSetState(&self.rasterizer);
            context.Draw(3, 0);

            // Unbind our SRV so the egui overlay / next frame does not see it still bound. Leave the
            // RTV bound: egui's own `render` binds the back-buffer RTV itself, and the present reads
            // the buffer regardless.
            context.PSSetShaderResources(0, Some(&[None]));
        }
    }

    /// Composite the egui panel texture over the back buffer as a flat, alpha-blended overlay inside
    /// `viewport` (letterboxed to the panel aspect), leaving the eye image visible around and through
    /// it. The eye letterbox draw must already have run; this does not clear. The caller must hold the
    /// context mutex.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context; `rtv`/`srv` must be live views over the
    /// game back buffer and the panel texture respectively.
    unsafe fn draw_overlay(
        &self,
        context: &ID3D11DeviceContext,
        rtv: &ID3D11RenderTargetView,
        srv: &ID3D11ShaderResourceView,
        viewport: Viewport,
    ) {
        unsafe {
            context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            context.OMSetBlendState(&self.overlay_blend, None, 0xffff_ffff);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: viewport.x,
                TopLeftY: viewport.y,
                Width: viewport.width,
                Height: viewport.height,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.PSSetShaderResources(0, Some(&[Some(srv.clone())]));
            context.RSSetState(&self.rasterizer);
            context.Draw(3, 0);

            // Restore opaque blending and unbind our SRV so the present / next frame does not see the
            // overlay state still bound.
            context.OMSetBlendState(None, None, 0xffff_ffff);
            context.PSSetShaderResources(0, Some(&[None]));
        }
    }

    /// Get (creating and caching on first use, rebuilding on a back-buffer change) the RTV over the
    /// game back buffer. Device methods are free-threaded.
    fn rtv_for(
        &mut self,
        device: &Device,
        back_texture: &jc3gi::graphics_engine::texture::ID3D11Resource,
    ) -> anyhow::Result<ID3D11RenderTargetView> {
        let ptr = back_texture.as_raw() as usize;
        if let Some((cached, rtv)) = self.rtv_cache.as_ref()
            && *cached == ptr
        {
            return Ok(rtv.clone());
        }
        let mut rtv: Option<ID3D11RenderTargetView> = None;
        unsafe {
            device
                .m_Device
                .CreateRenderTargetView(back_texture, None, Some(&mut rtv))
        }
        .context("vr: creating the game back-buffer render-target view")?;
        let rtv = rtv.context("vr: the game back-buffer render-target view was not created")?;
        self.rtv_cache = Some((ptr, rtv.clone()));
        Ok(rtv)
    }

    /// Get (creating and caching on first use, rebuilding on a texture change) the SRV over eye
    /// `eye`'s captured texture.
    fn srv_for(
        &mut self,
        device: &Device,
        eye: usize,
        src: &ID3D11Texture2D,
    ) -> anyhow::Result<ID3D11ShaderResourceView> {
        let ptr = src.as_raw() as usize;
        let cache = &mut self.srv_cache[eye];
        if let Some((cached, srv)) = cache.as_ref()
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
        .context("vr: creating the mirror eye shader-resource view")?;
        let srv = srv.context("vr: the mirror eye shader-resource view was not created")?;
        *cache = Some((ptr, srv.clone()));
        Ok(srv)
    }
}

#[cfg(test)]
mod tests;
