//! The HUD-redirect state machine: it lazily creates the target, applies and relinquishes the rebind,
//! owns the egui preview registration, and drives the lazy-follow damping for the floating panel.

mod resources;

use std::time::{Duration, Instant};

use glam::{Quat, Vec2, Vec3, Vec4};
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CLEAR_DEPTH, D3D11_CLEAR_STENCIL, ID3D11DeviceContext,
};

use crate::hud::{
    binding,
    config::FollowConfig,
    quad::{HudQuad, PanelParams},
    state::resources::{Native, Retry, Surface},
    target::HudTarget,
    world_markers::layout::StageFrame,
};

/// Global HUD state. Locked briefly on the render thread.
pub static HUD_STATE: Mutex<HudState> = Mutex::new(HudState::new());

/// What the applied redirect laid the HUD out for: the texture's size and whether the safe area
/// hugs the stage. Read by the capture seam, which must not take [`HUD_STATE`] for it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct AppliedRedirect {
    pub texture: (u32, u32),
    pub stage_safe_area: bool,
}

/// The applied redirect, or `None` while the HUD is not redirected.
pub(crate) fn applied_redirect() -> Option<AppliedRedirect> {
    *APPLIED_REDIRECT.lock()
}

/// Owned UI layout values copied at the main movie's capture boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct StageSafeArea {
    pub bounds: [f32; 4],
    pub percentage: f32,
    pub stage: Vec2,
    pub aspect: f32,
}

/// A main-movie capture sample; it contains no engine pointers or renderer resources.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct StageSafeAreaSample {
    pub at: Instant,
    pub layout: Option<StageSafeArea>,
}

/// Availability of the most recent safe-area publication.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StageSafeAreaReadout {
    Unavailable,
    Busy,
    Captured(StageSafeAreaSample),
}

impl StageSafeAreaSample {
    pub(crate) fn new(layout: StageSafeArea, at: Instant) -> Self {
        let [x1, y1, x2, y2] = layout.bounds;
        let valid = layout.bounds.into_iter().all(f32::is_finite)
            && x2 > x1
            && y2 > y1
            && layout.percentage.is_finite()
            && layout.stage.is_finite()
            && layout.stage.min_element() > 0.0
            && layout.aspect.is_finite()
            && layout.aspect > 0.0
            && StageFrame::new(layout.stage, layout.aspect)
                .is_some_and(|frame| frame.origin.is_finite() && frame.size.is_finite());
        Self {
            at,
            layout: valid.then_some(layout),
        }
    }

    pub(crate) fn fresh(self, now: Instant) -> bool {
        now.saturating_duration_since(self.at) < STAGE_SAFE_AREA_STALE
    }
}

/// Publish only owned values from the main movie's capture boundary.
pub(crate) fn publish_stage_safe_area(layout: StageSafeArea, at: Instant) {
    *STAGE_SAFE_AREA.lock() = Some(StageSafeAreaSample::new(layout, at));
}

/// Copy the publication without waiting for the capture producer or accessing the engine.
pub(crate) fn stage_safe_area_readout() -> StageSafeAreaReadout {
    copy_stage_safe_area(&STAGE_SAFE_AREA)
}

/// The live HUD-redirect state (render thread only, apart from the preview registration on the UI
/// thread).
pub struct HudState {
    target: Option<HudTarget>,
    retry: Retry,
    pending_unregisters: Vec<egui::TextureId>,
    /// Whether the redirect is currently applied to the UI's render buffer.
    redirected: bool,
    /// The last engine render size seen by [`ensure_redirected`]. A change means the engine re-ran
    /// `InitPlatformRT` (its device/reset path), rebuilding `m_RenderBuffer` from the engine surface
    /// and discarding our rebind -- so the redirect must be re-applied even though our target texture
    /// is independent of the engine's own targets.
    render_size: Option<(u32, u32)>,
    /// Whether the applied redirect computed the safe area against the stage's aspect (the atlas
    /// viewport probe). A change re-applies the redirect.
    stage_safe_area: bool,
    /// The egui texture id for the HUD preview, registered lazily on the UI thread.
    preview_id: Option<egui::TextureId>,
    /// The quad pass that draws the redirected HUD back into the scene, built lazily on first draw.
    quad: Option<HudQuad>,
    /// The dynamic-distance depth sampler, built lazily on the first enabled frame.
    depth_shift: Option<crate::hud::depth::DepthShift>,
    /// Lazy-follow damping state for the floating panel (gameplay HUD mode).
    follow: FollowState,
    /// The latched world-static pose `(position, rotation)` for [`HudMode::Movie`](crate::hud::HudMode::Movie):
    /// captured when the mode is entered and held so the panel does not move with the head. `None`
    /// outside movie mode.
    latched_pose: Option<(Vec3, Quat)>,
    /// The panel pose `(position, rotation)` chosen for the current frame, cached for the marker
    /// projection ([`compute_panel_vp`](crate::hud::compute_panel_vp)) to reuse.
    current_pose: Option<(Vec3, Quat)>,
    /// World-space panel corners, computed once per frame on eye 0 and reused for eye 1 so both
    /// eyes project the same world position through their own per-eye VP (correct stereo depth).
    cached_corners: Option<[Vec4; 4]>,
    /// The virtual mouse cursor's world-space corners, computed once per frame on eye 0 alongside
    /// the panel corners; `None` while the cursor is hidden (see [`crate::hud::cursor`]).
    cursor_corners: Option<[Vec4; 4]>,
}

/// Damped follow state: tracks the panel's eased orientation via quaternion slerp.
struct FollowState {
    /// Damped panel rotation (quaternion).
    rotation: Quat,
    /// Last frame's instant for delta-time computation.
    last_time: Option<Instant>,
}

impl FollowState {
    const fn new() -> Self {
        Self {
            rotation: Quat::IDENTITY,
            last_time: None,
        }
    }

    /// Ease the panel's rotation toward the head's current rotation using critically-damped
    /// slerp: `alpha = 1 - 2^(-dt/halflife); current = slerp(current, target, alpha)`. No deadzone
    /// -- the panel always follows, with the halflife controlling the lag. Returns the damped
    /// quaternion for the quad's world-space orientation.
    fn update(&mut self, head_rotation: Quat, config: &FollowConfig) -> Quat {
        let dt = self
            .last_time
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.016);
        self.last_time = Some(Instant::now());
        let dt = dt.min(0.1);

        let alpha = (1.0 - 2.0_f32.powf(-dt / config.rotation_halflife)).min(1.0);
        self.rotation = self.rotation.slerp(head_rotation, alpha);

        self.rotation
    }
}

impl HudState {
    const fn new() -> Self {
        Self {
            target: None,
            retry: Retry::new(),
            pending_unregisters: Vec::new(),
            redirected: false,
            render_size: None,
            stage_safe_area: false,
            preview_id: None,
            quad: None,
            depth_shift: None,
            follow: FollowState::new(),
            latched_pose: None,
            current_pose: None,
            cached_corners: None,
            cursor_corners: None,
        }
    }

    /// Ensure the HUD is redirected into a target at `texture_width` x `texture_height`, (re)creating
    /// the target on a config-size change and applying the rebind once. A render-size change
    /// (`render_width`/`render_height`) forces a re-apply without recreating the target,
    /// because the engine re-runs `InitPlatformRT` on a device/reset and discards our rebind. A failed
    /// allocation keeps an applied old target usable. A failed binding restores the engine binding
    /// and clears the applied publication. Failed requests retry at most once per second.
    /// `stage_safe_area` selects the atlas viewport probe's safe area (see
    /// [`binding::redirect_to`]); a change re-applies the redirect.
    pub(super) fn ensure_redirected(
        &mut self,
        device: &Device,
        texture_width: u32,
        texture_height: u32,
        render_width: u32,
        render_height: u32,
        stage_safe_area: bool,
    ) {
        let mut native = Native(device);
        let replaced = resources::redirect(
            Surface {
                target: &mut self.target,
                redirected: &mut self.redirected,
                render_size: &mut self.render_size,
                stage_safe_area: &mut self.stage_safe_area,
                retry: &mut self.retry,
            },
            &mut native,
            AppliedRedirect {
                texture: (texture_width, texture_height),
                stage_safe_area,
            },
            (render_width, render_height),
            Instant::now(),
        );
        if replaced {
            resources::retire(&mut self.preview_id, &mut self.pending_unregisters);
        }
        *APPLIED_REDIRECT.lock() = self.redirected.then(|| AppliedRedirect {
            texture: self
                .target
                .as_ref()
                .expect("a redirect owns its target")
                .size(),
            stage_safe_area: self.stage_safe_area,
        });
    }

    /// Restore the engine's own binding and drop our target, so the UI no longer renders into our
    /// texture. A no-op when not redirected. `render_width`/`render_height` are the engine render
    /// dimensions `InitPlatformRT` and the viewport expect.
    pub(super) fn restore(&mut self, render_width: u32, render_height: u32) {
        if self.redirected {
            binding::restore_engine_binding(render_width, render_height);
            self.redirected = false;
        }
        self.target = None;
        resources::retire(&mut self.preview_id, &mut self.pending_unregisters);
        self.retry.succeeded();
        *APPLIED_REDIRECT.lock() = None;
    }

    /// The frame's dynamic panel distance: dispatch the depth-histogram pass, pick up the async
    /// readback, and ease toward the policy's target (see [`crate::hud::depth`]). Falls back to
    /// `base` when the pipeline cannot be built. Call once per frame (eye 0) with the engine
    /// context mutex held.
    pub fn depth_distance(
        &mut self,
        context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
        device: &jc3gi::graphics_engine::device::Device,
        cfg: &crate::hud::config::DepthShiftConfig,
        mode: crate::hud::HudMode,
        base: f32,
    ) -> f32 {
        if self.depth_shift.is_none() {
            match crate::hud::depth::DepthShift::new(device) {
                Ok(shift) => self.depth_shift = Some(shift),
                Err(e) => {
                    // Log the build failure once, not per frame; the next attempt happens on the
                    // next frame regardless.
                    static LOGGED: std::sync::atomic::AtomicBool =
                        std::sync::atomic::AtomicBool::new(false);
                    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        tracing::error!("hud depth: {e:#}");
                    }
                    return base;
                }
            }
        }
        let Some(shift) = self.depth_shift.as_mut() else {
            return base;
        };
        // The histogram dispatch only runs for the histogram policy; the vehicle policy needs
        // no GPU work. The mask uses the previous frame's corners (this frame's depend on the
        // distance being computed) and the redirected HUD texture's alpha.
        if cfg.use_depth_histogram
            && let Some(graphics_engine) =
                // SAFETY: the graphics engine is the live singleton on the render thread.
                unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        {
            let hud_srv = self.target.as_ref().map(|t| t.color_srv().clone());
            let mask = match (
                cfg.mask_by_hud,
                self.cached_corners,
                self.current_pose,
                &hud_srv,
            ) {
                (true, Some(corners), Some((pos, _)), Some(srv)) => {
                    Some(crate::hud::depth::MaskInputs {
                        camera_pos: pos,
                        corners,
                        hud_srv: srv,
                    })
                }
                _ => None,
            };
            shift.sample(context, graphics_engine, cfg, mask);
        }
        shift.distance(cfg, mode, base)
    }

    /// The dynamic-distance state's live snapshot, for the debug UI.
    pub fn depth_status(&self) -> Option<crate::hud::depth::DepthShiftStatus> {
        self.depth_shift
            .as_ref()
            .map(crate::hud::depth::DepthShift::status)
    }

    /// Choose the panel pose `(position, rotation)` for the current frame from the head pose, caching
    /// it for the marker projection to reuse.
    ///
    /// When `freeze` is false the panel tracks the head: the position is the live head position and the
    /// rotation is the damped follow quaternion. When `freeze` is true (an in-game menu is open, see
    /// [`menu_world_lock`](crate::hud::menu_world_lock)) the pose is world-locked -- latched on the
    /// first frozen frame and held, so head movement no longer moves it -- and the latch is cleared the
    /// moment `freeze` drops, resuming head-follow.
    pub fn update_pose(
        &mut self,
        freeze: bool,
        head_pos: Vec3,
        head_rotation: Quat,
        follow: &FollowConfig,
    ) -> (Vec3, Quat) {
        let pose = if freeze {
            *self.latched_pose.get_or_insert((head_pos, head_rotation))
        } else {
            self.latched_pose = None;
            (head_pos, self.follow.update(head_rotation, follow))
        };
        self.current_pose = Some(pose);
        pose
    }

    /// The panel pose chosen for the current frame. Used by the W2S hook to project markers onto the
    /// panel's surface rather than the screen plane. `None` until [`update_pose`](HudState::update_pose)
    /// has run.
    pub fn panel_pose(&self) -> Option<(Vec3, Quat)> {
        self.current_pose
    }

    /// Compute the panel's world-space corners from the current camera and follow state, caching
    /// the result for both eyes. Call once per frame (eye 0); eye 1 reuses the cached corners.
    pub fn compute_world_corners(&mut self, params: &PanelParams) {
        self.cached_corners = None;
        if let Some(corners) = crate::hud::quad::compute_world_corners(params) {
            self.cached_corners = Some(corners);
        }
    }

    /// Set (or clear) the frame's cursor corners. Chosen on eye 0 alongside the panel corners.
    pub fn set_cursor_corners(&mut self, corners: Option<[Vec4; 4]>) {
        self.cursor_corners = corners;
    }

    /// Clear the cached panel geometry when the redirected panel is not being rendered.
    pub fn clear_panel_corners(&mut self) {
        self.cached_corners = None;
    }

    /// The world-space corners of the panel rendered this frame. The pointer uses this exact cache
    /// rather than reconstructing the pose, so world-lock, depth shifting, and follow damping cannot
    /// make the ray hit a different surface than the player sees.
    pub fn panel_corners(&self) -> Option<[Vec4; 4]> {
        self.cached_corners
    }

    /// The redirected HUD target, or `None` while the redirect is not applied.
    pub(super) fn redirected_target(&self) -> Option<&HudTarget> {
        self.target.as_ref().filter(|_| self.redirected)
    }

    /// The redirected texture's size, or `None` while the redirect is not applied. The mouse
    /// cursor's coordinate mapping rescales to this (movie rectangle = texture; see
    /// [`crate::hud::cursor`]).
    pub(super) fn redirected_size(&self) -> Option<(u32, u32)> {
        if !self.redirected {
            return None;
        }
        self.target.as_ref().map(HudTarget::size)
    }

    /// Draw the redirected HUD as a floating quad for `eye` over `target` (the eye's linear back
    /// buffer). A no-op when not redirected or when no cached corners are available (e.g. the camera
    /// was unavailable on eye 0). The quad pass is built lazily on first draw. The caller must hold
    /// the engine context mutex.
    pub fn draw_quad(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        target: &Texture,
        _eye: usize,
    ) {
        // Cinematic mode hides the HUD by skipping the quad: the redirect keeps drawing the game's
        // HUD into the offscreen texture, so flipping the mode back shows the live HUD at once.
        if crate::director::cinematic::active() {
            return;
        }
        self.draw_panel(context, device, target);
        self.draw_cursor(context, device, target);
    }

    /// Draw the panel surface: the whole HUD texture on one flat quad.
    fn draw_panel(&mut self, context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
        if !self.redirected {
            return;
        }
        let Some(corners) = self.cached_corners else {
            return;
        };
        let Some(hud_srv) = self.target.as_ref().map(|t| t.color_srv().clone()) else {
            return;
        };
        if self.quad.is_none() {
            match HudQuad::new(device) {
                Ok(quad) => self.quad = Some(quad),
                Err(e) => {
                    tracing::error!("HUD quad: {e:#}");
                    return;
                }
            }
        }
        if let Some(quad) = self.quad.as_ref() {
            quad.draw(context, device, target, &hud_srv, &corners);
        }
    }

    /// Draw the virtual mouse cursor over the panel. Drawn last: the cursor floats nearest the
    /// eye, and the quads are alpha-blended overlays ordered painter-style. A no-op while hidden
    /// or before the quad pipeline exists (it is built by the panel draw).
    fn draw_cursor(&mut self, context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
        if !self.redirected {
            return;
        }
        let Some(corners) = self.cursor_corners else {
            return;
        };
        if let Some(quad) = self.quad.as_ref() {
            quad.draw_cursor(context, device, target, &corners);
        }
    }

    /// Clear the HUD render target and depth-stencil so the next frame starts clean. A no-op when
    /// not redirected. The caller must hold the engine context mutex.
    pub fn clear(&mut self, context: &ID3D11DeviceContext) {
        if !self.redirected {
            return;
        }
        let Some(target) = self.target.as_ref() else {
            return;
        };
        // SAFETY: `context` is the live engine context; the RTV and DSV belong to our target
        // texture.
        unsafe {
            context.ClearRenderTargetView(target.color_rtv(), &[0.0, 0.0, 0.0, 0.0]);
            context.ClearDepthStencilView(
                target.depth_dsv(),
                (D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL).0,
                1.0,
                0,
            );
        }
    }

    /// Register (once) and return the egui texture id for previewing the redirected HUD. `None` until
    /// the HUD has been redirected into our texture.
    pub fn preview_id(
        &mut self,
        renderer: &mut egui_directx11::Renderer,
    ) -> Option<egui::TextureId> {
        self.drain_preview_retirements(renderer);
        if self.preview_id.is_none()
            && let Some(target) = self.target.as_ref()
        {
            self.preview_id = Some(renderer.register_user_texture(target.color_srv().clone()));
        }
        self.preview_id
    }

    /// Drop the egui preview registration. Call on the UI thread (it owns the renderer), so the texture
    /// id is released rather than leaked.
    pub fn release_preview(&mut self, renderer: &mut egui_directx11::Renderer) {
        resources::retire(&mut self.preview_id, &mut self.pending_unregisters);
        self.drain_preview_retirements(renderer);
    }

    pub(crate) fn drain_preview_retirements(&mut self, renderer: &mut egui_directx11::Renderer) {
        resources::drain(&mut self.pending_unregisters, |id| {
            renderer.unregister_user_texture(id);
        });
    }
}

/// The applied redirect, written by [`HudState`] on the render thread (a leaf lock).
static APPLIED_REDIRECT: Mutex<Option<AppliedRedirect>> = Mutex::new(None);

const STAGE_SAFE_AREA_STALE: Duration = Duration::from_secs(1);
static STAGE_SAFE_AREA: Mutex<Option<StageSafeAreaSample>> = Mutex::new(None);

fn copy_stage_safe_area(source: &Mutex<Option<StageSafeAreaSample>>) -> StageSafeAreaReadout {
    match source.try_lock() {
        Some(sample) => sample.map_or(
            StageSafeAreaReadout::Unavailable,
            StageSafeAreaReadout::Captured,
        ),
        None => StageSafeAreaReadout::Busy,
    }
}

#[cfg(test)]
mod test_stage_safe_area {
    use super::*;

    #[test]
    fn publication_copy_is_nonblocking_and_preserves_owned_samples() {
        let source = Mutex::new(None);
        assert_eq!(
            copy_stage_safe_area(&source),
            StageSafeAreaReadout::Unavailable
        );
        let sample = StageSafeAreaSample::new(layout(), Instant::now());
        *source.lock() = Some(sample);
        assert_eq!(
            copy_stage_safe_area(&source),
            StageSafeAreaReadout::Captured(sample)
        );
        let _guard = source.lock();
        assert_eq!(copy_stage_safe_area(&source), StageSafeAreaReadout::Busy);
    }

    #[test]
    fn capture_freshness_expires_at_the_boundary() {
        let at = Instant::now();
        let sample = StageSafeAreaSample::new(layout(), at);
        assert!(sample.fresh(at));
        assert!(sample.fresh(at + STAGE_SAFE_AREA_STALE - Duration::from_nanos(1)));
        assert!(!sample.fresh(at + STAGE_SAFE_AREA_STALE));
    }

    #[test]
    fn invalid_capture_values_do_not_publish_a_layout() {
        let original = layout();
        let invalid = [
            StageSafeArea {
                bounds: [f32::NAN, 0.0, 1.0, 1.0],
                ..original
            },
            StageSafeArea {
                bounds: [0.0, 0.0, 0.0, 1.0],
                ..original
            },
            StageSafeArea {
                bounds: [0.0, 1.0, 1.0, 0.0],
                ..original
            },
            StageSafeArea {
                percentage: f32::INFINITY,
                ..original
            },
            StageSafeArea {
                stage: Vec2::ZERO,
                ..original
            },
            StageSafeArea {
                stage: Vec2::splat(f32::NAN),
                ..original
            },
            StageSafeArea {
                aspect: 0.0,
                ..original
            },
            StageSafeArea {
                aspect: f32::INFINITY,
                ..original
            },
            StageSafeArea {
                aspect: f32::MIN_POSITIVE,
                ..original
            },
        ];
        for layout in invalid {
            assert!(
                StageSafeAreaSample::new(layout, Instant::now())
                    .layout
                    .is_none()
            );
        }
        assert_eq!(
            StageSafeAreaSample::new(original, Instant::now()).layout,
            Some(original)
        );
    }

    fn layout() -> StageSafeArea {
        StageSafeArea {
            bounds: [32.0, 18.0, 1248.0, 702.0],
            percentage: 0.05,
            stage: Vec2::new(1280.0, 720.0),
            aspect: 1.0,
        }
    }
}
