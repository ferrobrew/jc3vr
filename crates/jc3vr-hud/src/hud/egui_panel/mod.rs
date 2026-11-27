//! The interactive egui debug panel, floating as a 2D surface in 3D space in VR (issue #24).
//!
//! In VR the flat egui overlay that rides the game back buffer is invisible -- the desktop cursor
//! lands in neither eye and the overlay is suppressed by `BLOCK_FLIP` -- so the debug UI is redirected
//! into an offscreen texture and drawn back into the scene as a floating quad, with the gameplay
//! HUD's quad pipeline (see `crate::hud::quad`), and driven by the desktop mouse re-sourced onto the
//! panel surface or by either controller's aim ray (see [`crate::hud::pointer`]). Each frame's corners
//! are published with the camera basis that placed them, for the controller pointer to intersect,
//! and the cursor dot follows whichever source owns the pointer.
//!
//! The panel lives in the player's play space, like a window in a VR runtime's dashboard: its pose
//! is held in the cockpit frame, the tracked space the head and the hands are composed in, so it
//! travels with the player and stays where it was left as the head turns (`slot`). A hand grabs
//! it with the grip and carries it, and the thumbstick pushes it along the aim ray while it is held
//! ([`crate::hud::pointer`]). Its world corners are composed at render time through the rendered
//! camera's basis, so it does not trail the head when the player moves fast.
//!
//! This is a wholly independent panel: its own render target, its own placement, and its own
//! eye-0-cached corners, so it never touches [`crate::hud::state::HUD_STATE`] and cannot perturb the
//! gameplay HUD. The whole feature is gated behind [`is_active`] -- an OpenXR session running *and*
//! the opt-in [`EguiPanelConfig::enabled`] flag -- and defaults off, so with it off every path is
//! byte-identical to the flat overlay baseline.
//!
//! egui is tessellated exactly once per frame (the single-use `RendererOutput`), on eye 0, into the
//! panel texture; both eyes then draw the same cached world-space corners through their own per-eye
//! VP (the same compute-on-eye-0 rule as the gameplay HUD), and the desktop mirror composites the
//! same panel texture (through the HUD's [`MirrorLayers`](jc3vr_view::vr::MirrorLayers)
//! registration). There is never a second egui pass.

mod placement;
mod slot;

use std::sync::atomic::{AtomicBool, Ordering};

use glam::Vec4;
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use jc3vr_input::{
    capture::{PanelRequest, debug_ui_capture},
    frame,
    frame::{Hand, Pose},
};
use jc3vr_view::{
    hud::panel::{RenderedPanel, publish_egui_panel, rendered_camera},
    vr::EguiPanelConfig,
};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11ShaderResourceView};

pub use crate::hud::egui_panel::slot::{end_grab, is_placed, recall};
use crate::{
    egui_impl::{EguiState, request_debug_ui},
    hud::{
        config::HUD_CONFIG,
        cursor::CursorFrame,
        egui_panel::{
            placement::world_panel,
            slot::{ResolveInput, Summon},
        },
        pointer,
        quad::{HudQuad, compute_cursor_corners, compute_world_corners},
        target::HudTarget,
        tuning::HUD,
    },
};

/// The panel pipeline and per-frame cache. Locked briefly on the render thread (and, for the SRV
/// handoff, on the game thread from the mirror).
static EGUI_PANEL: Mutex<EguiPanelState> = Mutex::new(EguiPanelState::new());

/// Release the panel texture and quad pipeline, for eject.
pub fn release_on_eject() {
    *EGUI_PANEL.lock() = EguiPanelState::new();
}

/// Whether the panel was active last frame, so that showing it and dismissing it each act once, on
/// their edge.
static WAS_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether the egui floating panel is active this frame: the opt-in panel is enabled, the clean
/// view does not hide everything, and a VR frame is rendering. The single predicate gating every
/// panel path; off is the untouched baseline.
///
/// This runs on the render thread (per-eye `draw_quad`), which must never block on the VR runtime's
/// locks: the frame holds the submit side across the eye draws (`FrameContext`), so a blocking lock
/// on the runtime here would deadlock the draw against `WaitForCPUDrawToFinish`. The enabled flag
/// is checked first (cheap, and [`HUD_CONFIG`] is never held across a draw), and the VR-frame
/// signal is the draw-safe [`jc3vr_view::vr::render_params`] slot (a separate lock, published per
/// frame precisely so draw-thread hooks can read it), not the runtime lock.
pub fn is_active() -> bool {
    HUD_CONFIG.query(|c| c.egui_panel.enabled)
        && !jc3vr_core::clean_view::hides_everything()
        && jc3vr_view::vr::render_params(0).is_some()
}

/// Show or hide the panel, for the F8 key and the controller shortcut. Reopening enters editing; a
/// visible read-only panel cannot edit itself.
pub fn toggle() -> bool {
    let enabled = {
        let mut hud = HUD_CONFIG.edit();
        hud.egui_panel.enabled = !hud.egui_panel.enabled;
        hud.egui_panel.enabled
    };
    let request = if enabled {
        PanelRequest::Open
    } else {
        PanelRequest::Hide
    };
    if let Some(egui_state) = EguiState::get().as_mut() {
        if enabled {
            egui_state.request_capture(PanelRequest::Hide);
        }
        egui_state.request_capture(request);
    }
    enabled
}

/// Hide the current debug surface without changing the independent desktop/panel preference.
/// This queues ownership work because the top-bar callback holds the egui state lock.
pub fn hide() {
    if is_active() {
        HUD_CONFIG.edit().egui_panel.enabled = false;
    } else {
        jc3vr_core::clean_view::hide();
    }
    request_debug_ui(PanelRequest::Hide);
}

/// Whether the panel may accept controller pointer, scroll, and grab input.
pub(crate) fn accepts_edit_input() -> bool {
    is_active() && debug_ui_capture().is_some_and(|snapshot| snapshot.edit_capture.pointer)
}

pub(crate) fn begin_grab(hand: Hand, aim: Pose) -> bool {
    accepts_edit_input() && slot::begin_grab(hand, aim)
}

pub(crate) fn hold_grab(hand: Hand, aim: Option<Pose>, rate: f32, dt: f32) {
    if accepts_edit_input() {
        slot::hold_grab(hand, aim, rate, dt);
    } else {
        slot::end_grab();
    }
}

/// The panel texture size when the panel is active, or `None` otherwise. The egui layout is sized to
/// this ([`crate::egui_impl::EguiState::set_panel_mode`]) and the pointer maps into it.
pub fn active_size() -> Option<(u32, u32)> {
    is_active().then(|| HUD_CONFIG.query(|c| c.egui_panel.resolution))
}

/// A clone of the panel texture's shader-resource view for the desktop mirror composite, or `None`
/// while the panel target does not exist yet. Locks [`EGUI_PANEL`] briefly.
pub(crate) fn panel_srv() -> Option<ID3D11ShaderResourceView> {
    EGUI_PANEL
        .lock()
        .target
        .as_ref()
        .map(|t| t.color_srv().clone())
}

/// Draw the egui panel for `eye` over `target` (the eye's linear back buffer). On eye 0 -- and only
/// while [`is_active`] -- the egui frame is rendered into the panel texture, the placement is settled,
/// and the world-space corners (panel + cursor) are computed and cached. Both eyes then draw the
/// cached corners. A no-op while inactive. Called from the render-thread post-draw hook after the
/// gameplay HUD quad, with the engine context mutex held.
pub(crate) fn draw_quad(
    context: &ID3D11DeviceContext,
    device: &Device,
    target: &Texture,
    eye: usize,
) {
    if !is_active() {
        if WAS_ACTIVE.swap(false, Ordering::Relaxed) {
            publish_egui_panel(None);
        }
        return;
    }
    // Shown again, the panel keeps where it was left, unless that is behind the player.
    if !WAS_ACTIVE.swap(true, Ordering::Relaxed) {
        slot::summon(Summon::Shown);
    }
    let cfg = HUD_CONFIG.lock().egui_panel;
    let mut panel = EGUI_PANEL.lock();
    if jc3vr_view::stereo::dispatch::first_ui_eye(eye) {
        panel.prepare(context, device, &cfg);
    }
    panel.draw(context, device, target);
}

/// The panel's render target, quad pipeline, and eye-0 corner cache.
struct EguiPanelState {
    target: Option<HudTarget>,
    quad: Option<HudQuad>,
    /// World-space panel corners, computed on eye 0 and reused for eye 1 (correct stereo depth).
    cached_corners: Option<[Vec4; 4]>,
    /// The virtual cursor's world-space corners, computed on eye 0 alongside the panel corners.
    cursor_corners: Option<[Vec4; 4]>,
}

impl EguiPanelState {
    const fn new() -> Self {
        Self {
            target: None,
            quad: None,
            cached_corners: None,
            cursor_corners: None,
        }
    }

    /// Eye-0 setup: (re)create the panel target at the configured resolution, render this frame's egui
    /// output into it, settle the placement, compute + cache the panel and cursor corners, and
    /// publish the corners for the controller pointer.
    fn prepare(&mut self, context: &ID3D11DeviceContext, device: &Device, cfg: &EguiPanelConfig) {
        self.cached_corners = None;
        self.cursor_corners = None;
        publish_egui_panel(None);

        let (width, height) = (cfg.resolution.0.max(1), cfg.resolution.1.max(1));
        if self.target.as_ref().map(HudTarget::size) != Some((width, height)) {
            match HudTarget::new(device, width, height) {
                Ok(target) => {
                    // A new texture's contents are undefined, and `render_to` leaves the target
                    // as it is when this frame's egui output was already taken.
                    // SAFETY: the caller holds the engine context mutex; the view is live.
                    unsafe { context.ClearRenderTargetView(target.color_rtv(), &[0.0; 4]) };
                    self.target = Some(target);
                }
                Err(e) => {
                    tracing::error!("egui panel target: {e:#}");
                    self.target = None;
                    return;
                }
            }
        }
        let Some(target) = self.target.as_ref() else {
            return;
        };

        // Render the single-use egui output into the panel texture. A no-op if the output was already
        // consumed this frame (e.g. the flat path ran first), which leaves the previous panel content.
        if let Some(egui_state) = EguiState::get().as_mut() {
            egui_state.render_to(context, target.color_rtv());
        }

        // The panel is placed in the cockpit frame and composed into the world through the
        // pointer's captured camera, the render-time head with the tracking sample that placed it,
        // so the panel, the head, and the controller rays share one basis.
        let Some(camera) = rendered_camera() else {
            return;
        };
        let tuning = &HUD.get().egui_panel;
        let held = slot::grabbing().and_then(|hand| Some((hand, latest_aim(hand)?)));
        let pose = slot::resolve(ResolveInput {
            head: camera.cockpit_head,
            distance: tuning.summon_distance_m,
            held,
            recenters: jc3vr_view::vr::recenter_count(),
        });
        let aspect = cfg.aspect.max(f32::EPSILON);
        // The panel's physical size is the size that gives the configured apparent size at the
        // summoning distance, so pushing it away shrinks it in view as a real window would.
        let panel_height = crate::hud::panel_height(tuning.scale, tuning.summon_distance_m, aspect);
        let params = world_panel(&camera, pose, panel_height, aspect);
        self.cached_corners = compute_world_corners(&params);
        publish_egui_panel(self.cached_corners.map(|corners| RenderedPanel {
            head: camera.head,
            cockpit_head: camera.cockpit_head,
            scale: camera.scale,
            corners,
        }));
        self.cursor_corners = accepts_edit_input()
            .then(pointer::egui_cursor_uv)
            .flatten()
            .and_then(|(u, v)| compute_cursor_corners(&params, CursorFrame { u, v }));
    }

    /// Draw the cached panel and cursor corners over `target` for the current eye. A no-op until the
    /// pipeline, target, and corners exist. The quad pipeline is built lazily on the first draw.
    fn draw(&mut self, context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
        let Some(corners) = self.cached_corners else {
            return;
        };
        let Some(srv) = self.target.as_ref().map(|t| t.color_srv().clone()) else {
            return;
        };
        if self.quad.is_none() {
            match HudQuad::new(device) {
                Ok(quad) => self.quad = Some(quad),
                Err(e) => {
                    tracing::error!("egui panel quad: {e:#}");
                    return;
                }
            }
        }
        let Some(quad) = self.quad.as_ref() else {
            return;
        };
        quad.draw(context, device, target, &srv, &corners);
        if let Some(cursor_corners) = self.cursor_corners {
            quad.draw_cursor(context, device, target, &cursor_corners);
        }
    }
}

/// The latest cockpit-frame aim pose of `hand`, sampled for this frame, or `None` while it is not
/// tracked. The controller frame slot is a leaf, taken and released here before the placement's.
fn latest_aim(hand: Hand) -> Option<Pose> {
    frame::with_latest(|frame| {
        let side = frame.hand(hand);
        (frame.focused && side.active && side.aim.is_valid()).then_some(side.aim.pose)
    })
    .flatten()
}
