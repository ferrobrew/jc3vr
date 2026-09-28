//! The world-space overlay layer: the plan of record's "The world-space overlay layer"
//! (`docs/mod/input/controllers-and-roomscale.md`). Sprites and lines drawn into the scene per eye,
//! tested against the scene's depth, for readouts that need their own depths: the grapple's
//! highlight and beam, the guns' dot or laser, the grab-volume highlights, the ghosts of yielded
//! hands, and the controller gizmos.
//!
//! - [`primitive`]: what a producer submits;
//! - [`store`]: the retained per-source slots, replaced by each submission and expiring on their
//!   own, which any thread may submit to ([`store::submit`], [`store::clear`]);
//! - [`producers`]: the overlay's own producers, run once per rendered frame;
//! - [`resolve`]: anchors placed and primitives packed for the frame, relative to its origin;
//! - [`view`]: each eye's camera-relative view, in two-pass stereo and under the collapse;
//! - [`renderer`]: the D3D11 pipeline.
//!
//! The overlay draws at the end of `RenderEngine::PostDraw`, onto the eye's final back buffer
//! before the HUD panel, after the post chain and the upscaler: the readouts keep their colours
//! through exposure changes and do not smear under temporal reconstruction. The frame's
//! primitives are snapshotted and uploaded once, on the first eye's draw, so both eyes draw the same
//! geometry.

pub mod primitive;
pub mod producers;
pub mod renderer;
pub mod resolve;
pub mod store;
pub mod view;

use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use jc3gi::graphics_engine::{device::Device, graphics_engine::GraphicsEngine, texture::Texture};
use parking_lot::Mutex;
use windows::{
    Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext},
    core::Interface as _,
};

use crate::{
    config::Config,
    controllers::{
        config::OverlayConfig,
        frame::{Hand, Pose},
        hands::late,
        overlay::{
            primitive::{Primitive, Source},
            renderer::{EyeConstants, EyeDraw, Renderer},
            resolve::{Frame, GpuPrimitive, HandPoses},
            view::EyeView,
        },
        scheme,
    },
    hooks,
};

/// The overlay's readout for the debug UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OverlayStatus {
    /// Each source's live primitives at the last snapshot, indexed by [`Source::index`].
    pub counts: [usize; Source::COUNT],
    /// The primitives drawn per eye in the last frame.
    pub drawn: u32,
    /// Frames the overlay drew anything in, since the payload loaded.
    pub frames: u64,
    /// The last failure to build or draw the pipeline.
    pub error: Option<String>,
}

/// The latest readout.
pub fn status() -> OverlayStatus {
    STATUS.lock().clone()
}

/// Run the overlay's producers. Called once per render dispatch at the end of
/// `RenderEngine::PostDraw`, before the engine's context lock is taken; it acts only on the
/// frame's first eye.
pub(crate) fn tick(eye: usize) {
    if eye != 0 || crate::stereo::far_phase() {
        return;
    }
    let active = Config::lock_query(|c| c.controllers.enabled && c.controllers.overlay.enabled)
        && scheme::is_controllers()
        && hooks::in_gameplay();
    producers::tick(active, &render_poses());
}

/// Draw the overlay onto `target` (the eye's final back buffer) for `eye`. The caller holds the
/// engine's context lock and, under the collapse, has selected the eye with
/// `set_collapse_ui_eye`.
pub(crate) fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    if crate::stereo::far_phase() || crate::director::cinematic::active() {
        return;
    }
    let (enabled, config) = Config::lock_query(|c| {
        (
            c.controllers.enabled && c.controllers.overlay.enabled,
            c.controllers.overlay,
        )
    });
    if !enabled {
        STATUS.lock().drawn = 0;
        return;
    }
    let frame = if eye == 0 { Some(frame_inputs()) } else { None };
    let camera = view::current(eye);
    // SAFETY: the engine singleton and its main depth texture are live while the engine renders.
    let depth = unsafe { GraphicsEngine::get().and_then(|g| g.m_MainDepthTexture.as_ref()) };

    let mut state = STATE.lock();
    let state = &mut *state;
    if state.failed {
        return;
    }
    let d3d = &device.m_Device;
    if state.renderer.is_none() {
        match Renderer::new(d3d) {
            Ok(renderer) => state.renderer = Some(renderer),
            Err(error) => {
                tracing::error!("{error:#}");
                state.failed = true;
                STATUS.lock().error = Some(format!("{error:#}"));
                return;
            }
        }
    }
    let Some(renderer) = state.renderer.as_mut() else {
        return;
    };

    if let Some(frame) = frame {
        let counts = store::snapshot(&mut state.primitives);
        match frame {
            Some(frame) => {
                resolve::resolve(&state.primitives, &frame, &mut state.packed);
                state.origin = frame.origin;
            }
            None => state.packed.clear(),
        }
        let uploaded = renderer.upload(d3d, context, &state.packed);
        let mut status = STATUS.lock();
        status.counts = counts;
        status.drawn = renderer.count();
        if renderer.count() > 0 {
            status.frames += 1;
        }
        if let Err(error) = uploaded {
            report(&mut status, error);
        }
    }
    if renderer.count() == 0 {
        return;
    }

    let (Some((view, viewport)), Some(depth)) = (camera, depth) else {
        return;
    };
    if depth.m_SRV.as_raw().is_null() {
        return;
    }
    let Some(eye_draw) = eye_draw(target, depth, &view, viewport, state.origin, &config) else {
        return;
    };
    if let Err(error) = renderer.draw(d3d, context, &eye_draw) {
        report(&mut STATUS.lock(), error);
    }
}

struct State {
    renderer: Option<Renderer>,
    /// Set when the pipeline could not be built, so that it is not retried every frame.
    failed: bool,
    /// The frame's snapshot of the submitted primitives.
    primitives: Vec<Primitive>,
    /// The frame's packed primitives.
    packed: Vec<GpuPrimitive>,
    /// The frame's origin, which the packed positions are relative to.
    origin: Vec3,
}

static STATE: Mutex<State> = Mutex::new(State {
    renderer: None,
    failed: false,
    primitives: Vec::new(),
    packed: Vec::new(),
    origin: Vec3::ZERO,
});

static STATUS: Mutex<OverlayStatus> = Mutex::new(OverlayStatus {
    counts: [0; Source::COUNT],
    drawn: 0,
    frames: 0,
    error: None,
});

/// Whether a draw failure has been logged; later ones are only recorded in the status.
static DRAW_ERROR_LOGGED: AtomicBool = AtomicBool::new(false);

/// Each hand's render-time world poses at the camera's interpolation fraction.
fn render_poses() -> [Option<HandPoses>; 2] {
    let dtf = hooks::camera::last_dtf();
    let hands = late::render_hands();
    Hand::BOTH.map(|hand| {
        hands[hand.index()].map(|h| HandPoses {
            wrist: h.wrist.at(dtf),
            grip: h.grip.at(dtf),
            aim: h.aim.at(dtf),
        })
    })
}

/// The frame's resolution inputs, with the head as the origin; `None` when no camera is readable.
fn frame_inputs() -> Option<Frame> {
    let (position, orientation) = crate::hud::render_camera_pose()?;
    Some(Frame {
        origin: position,
        head: Pose {
            position,
            orientation,
        },
        hands: render_poses(),
    })
}

/// One eye's draw inputs: its viewport (the collapse's half, or the whole target) and constants.
fn eye_draw<'a>(
    target: &'a Texture,
    depth: &'a Texture,
    view: &EyeView,
    viewport: Option<D3D11_VIEWPORT>,
    origin: Vec3,
    config: &OverlayConfig,
) -> Option<EyeDraw<'a>> {
    let (target_w, target_h) = (f32::from(target.m_Width), f32::from(target.m_Height));
    let (depth_w, depth_h) = (f32::from(depth.m_Width), f32::from(depth.m_Height));
    if target_w < 1.0 || target_h < 1.0 || depth_w < 1.0 || depth_h < 1.0 {
        return None;
    }
    let viewport = viewport.unwrap_or(D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: target_w,
        Height: target_h,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    });
    let (a, b) = view.z_row();
    Some(EyeDraw {
        target,
        scene_depth: &depth.m_SRV,
        viewport,
        constants: EyeConstants {
            offset_view_projection: view.offset_view_projection.to_cols_array(),
            origin_to_eye: (origin - view.position).extend(0.0).to_array(),
            viewport_size: [viewport.Width, viewport.Height],
            pixel_scale: view.x_scale() * viewport.Width * 0.5,
            min_line_pixels: config.min_line_px,
            depth_scale: [depth_w / target_w, depth_h / target_h],
            depth_z_row: [a, b],
            depth_max: [depth_w - 1.0, depth_h - 1.0],
            depth_tolerance_abs: config.depth_tolerance_m,
            depth_tolerance_rel: config.depth_tolerance_rel,
        },
    })
}

fn report(status: &mut OverlayStatus, error: anyhow::Error) {
    if !DRAW_ERROR_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::error!("{error:#}");
    }
    status.error = Some(format!("{error:#}"));
}
