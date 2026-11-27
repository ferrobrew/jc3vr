//! The world-space overlay's draw: the frame's retained primitives snapshotted, resolved, and
//! uploaded on the first eye, and drawn onto each eye's final back buffer, tested against the
//! scene's depth.
//!
//! The draw reads what it needs from the controllers through the [`OverlayInputs`] they register
//! ([`jc3vr_view::controllers::overlay::inputs`]).

use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec3;
use jc3gi::graphics_engine::{device::Device, graphics_engine::GraphicsEngine, texture::Texture};
use jc3vr_core::hooks::in_gameplay;
use jc3vr_input::frame::Pose;
use jc3vr_view::controllers::overlay::{
    inputs,
    inputs::{DrawSettings, OverlayInputs},
    primitive::{Primitive, Source},
    resolve,
    resolve::{Frame, GpuPrimitive},
    store,
};
use parking_lot::Mutex;
use windows::{
    Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext},
    core::Interface as _,
};

use crate::controllers::overlay::{
    renderer::{EyeConstants, EyeDraw, Renderer},
    view,
    view::EyeView,
};

/// The overlay's readout for the debug UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OverlayStatus {
    /// Each source's live retained primitives before suppression and the cap, at the last snapshot.
    pub counts: [usize; Source::COUNT],
    /// Each source's primitives collected after suppression and the cap, before resolution.
    pub visible_counts: [usize; Source::COUNT],
    /// Each source's live primitives omitted by the temporary output mask, before the cap.
    pub suppressed_counts: [usize; Source::COUNT],
    /// The primitives drawn per eye in the last frame.
    pub drawn: u32,
    /// Frames the overlay drew anything in, since the payload loaded.
    pub frames: u64,
    /// The last failure to build or draw the pipeline.
    pub error: Option<String>,
}

/// Release the pipeline, for eject: it holds a render-target view over the engine's back buffer,
/// which keeps that texture alive after the engine resizes off it.
pub fn release_on_eject() {
    STATE.lock().renderer = None;
}

/// The latest readout.
pub fn status() -> OverlayStatus {
    STATUS.lock().clone()
}

/// Draw the overlay onto `target` (the eye's final back buffer) for `eye`. The caller holds the
/// engine's context lock and, under the collapse, has selected the eye with
/// `set_collapse_ui_eye`.
pub fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    if jc3vr_core::clean_view::hides_everything() {
        return;
    }
    let Some(inputs) = inputs::registered() else {
        return;
    };
    let Some(settings) = (inputs.settings)() else {
        STATUS.lock().drawn = 0;
        return;
    };
    let frame = jc3vr_view::stereo::dispatch::first_ui_eye(eye).then(|| frame_inputs(inputs));
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
        let collection = store::snapshot(&mut state.primitives, in_gameplay());
        match frame {
            Some(frame) => {
                resolve::resolve(&state.primitives, &frame, &mut state.packed);
                state.origin = frame.origin;
            }
            None => state.packed.clear(),
        }
        let uploaded = renderer.upload(d3d, context, &state.packed);
        let mut status = STATUS.lock();
        status.counts = collection.retained;
        status.visible_counts = collection.visible;
        status.suppressed_counts = collection.suppressed;
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
    let Some(eye_draw) = eye_draw(target, depth, &view, viewport, state.origin, &settings) else {
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
    visible_counts: [0; Source::COUNT],
    suppressed_counts: [0; Source::COUNT],
    drawn: 0,
    frames: 0,
    error: None,
});

/// Whether a draw failure has been logged; later ones are only recorded in the status.
static DRAW_ERROR_LOGGED: AtomicBool = AtomicBool::new(false);

/// The frame's resolution inputs, with the head as the origin; `None` when no camera is readable.
/// The head-anchored primitives ride the render camera, whose position is the eye midpoint the
/// tick measured their offsets from ([`jc3vr_view::hooks::camera::eye_point`]).
fn frame_inputs(inputs: &OverlayInputs) -> Option<Frame> {
    let (position, orientation) = jc3vr_view::hud::render_camera_pose()?;
    Some(Frame {
        origin: position,
        head: Pose {
            position,
            orientation,
        },
        hands: (inputs.hand_poses)(),
    })
}

/// One eye's draw inputs: its viewport (the collapse's half, or the whole target) and constants.
fn eye_draw<'a>(
    target: &'a Texture,
    depth: &'a Texture,
    view: &EyeView,
    viewport: Option<D3D11_VIEWPORT>,
    origin: Vec3,
    settings: &DrawSettings,
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
            min_line_pixels: settings.min_line_px,
            depth_scale: [depth_w / target_w, depth_h / target_h],
            depth_z_row: [a, b],
            depth_max: [depth_w - 1.0, depth_h - 1.0],
            depth_tolerance_abs: settings.depth_tolerance_m,
            depth_tolerance_rel: settings.depth_tolerance_rel,
        },
    })
}

fn report(status: &mut OverlayStatus, error: anyhow::Error) {
    if !DRAW_ERROR_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::error!("{error:#}");
    }
    status.error = Some(format!("{error:#}"));
}
