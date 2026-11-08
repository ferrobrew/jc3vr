//! Floating-HUD redirect: render the game's HUD into our own offscreen texture.
//!
//! The Scaleform UI normally renders into the engine's working surface, so it composites onto the
//! scene at the screen plane. Step one of the floating HUD is to redirect it into a texture we own, by
//! rebinding the UI's render buffer ([`UIManager::m_RenderBuffer`]) to a render-target view over our
//! texture via [`RenderTargetData::UpdateData`]. Once redirected, the HUD no longer lands on the
//! working surface, so it drops out of the scene composite automatically; we later draw our texture as
//! a 3D quad.
//!
//! The rebind is not tied to startup, so under late injection (where `InitPlatformRT` has already
//! created `m_RenderBuffer`) we just call `UpdateData` once. We install lazily on the render thread and
//! re-apply on a resolution change, the same compare-and-recreate pattern the FSR and debug captures
//! use. Disabling the redirect (or unloading) restores the engine's own binding by re-running
//! [`UIManager::InitPlatformRT`], so the UI never renders into a freed texture.
//!
//! `InitPlatformRT` rebinds GPU views, so the redirect and the restore both run from the per-frame
//! [`tick`] on the render thread. Shutdown (game thread) just clears the config flag and lets a few
//! more frames tick before the hooks come down, so the restore happens on the render thread the same
//! way a toggle-off does.
//!
//! The panel has two modes (see [`HudMode`], chosen per frame by [`current_mode`]): the gameplay HUD
//! lazily follows the head's orientation with critically-damped quaternion slerp, while full-screen
//! UI (movies, loading screens, menus) is world-static -- latched in place so the head can look away
//! from it. Each mode also has its own aspect ([`HUD_ASPECT`] / [`MOVIE_ASPECT`]).
//! The panel's world-space corners are computed once per frame (eye 0) and projected through each
//! eye's own per-eye VP, so it sits at a finite world position with correct stereo depth rather than
//! being head-locked at infinity.
//!
//! The module is split into the GPU resources ([`target`]), the game-side UI rebind operations
//! ([`binding`]), the quad draw pass ([`quad`]), the [`state`] machine that drives them, and the
//! [`config`] types for tuning parameters. Parts of the gameplay HUD leave the panel through the
//! [`atlas`]: the [`world_markers`] at their world points, the [`anchored`] readouts at the
//! gun hand and the off-hand's wrist, and the reticles at the aim point ([`reticle`]).
//! The [`grab_hint`] draws the game's icons ([`game_icons`]) on the panel's plane, and
//! [`lost_weapons`] over the weapons the player let go of; [`mark_labels`] names the grip markers
//! that carry a label, such as the nitrous button.

pub mod aim;
pub(crate) mod anchored;
pub(crate) mod atlas;
mod binding;
pub(crate) mod config;
pub mod cursor;
pub mod depth;
pub(crate) mod egui_panel;
pub(crate) mod game_icons;
pub(crate) mod grab_hint;
mod label;
mod log_once;
pub(crate) mod lost_weapons;
pub(crate) mod mark_labels;
pub(crate) mod mirror_overlay;
mod offscreen_egui;
mod palette;
pub(crate) mod pointer;
mod quad;
pub(crate) mod restart_notice;
mod reticle;
pub mod scaleform;
mod state;
pub(crate) mod summary_panel;
mod target;
pub(crate) mod world_markers;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

pub use config::HudConfig;
use glam::{Mat3, Mat4, Quat, Vec2, Vec3};
use jc3gi::{
    camera::camera_manager::CameraManager,
    graphics_engine::{device::Device, texture::Texture},
    types::math::Matrix4,
    ui::ui_manager::{GetIUIManager, UIManager},
};
pub use state::HUD_STATE;
use state::{StageSafeArea, publish_stage_safe_area};
pub(crate) use state::{StageSafeAreaReadout, stage_safe_area_readout};
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    config::{Config, Switch, disable_automatically},
    hooks::in_gameplay,
    stereo::single_pass::collapse_active,
};

/// Which presentation the HUD is in this frame. Drives the panel's aspect (gameplay vs full-screen
/// UI). Placement (head-following vs world-locked) is chosen separately by
/// [`menu_world_lock`], which only freezes for in-game menus with a valid camera.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HudMode {
    /// Gameplay HUD: rendered at [`HUD_ASPECT`], head-following.
    Hud,
    /// Full-screen UI -- movies, loading screens, and menus: rendered at [`MOVIE_ASPECT`].
    Movie,
}

/// Determine the current [`HudMode`] from the game state and UI. Any state other than in-game play
/// ([`crate::hooks::in_gameplay`] is false -- so loading screens, the frontend, and install/startup) is
/// full-screen, as is an in-game modal menu (the pause / mission / reward screens, detected via the
/// UI's static background grab).
pub fn current_mode() -> HudMode {
    if !in_gameplay() {
        return HudMode::Movie;
    }
    // SAFETY: GetIUIManager returns the live UI singleton (or null before it exists);
    // IsUsingStaticBackGround is a const getter.
    let static_background = unsafe {
        GetIUIManager()
            .as_ref()
            .is_some_and(|ui| ui.IsUsingStaticBackGround())
    };
    if static_background {
        return HudMode::Movie;
    }
    HudMode::Hud
}

/// Whether the mod's quads placed in the world (the anchored readouts, the grab hint, the lost
/// weapon markers, and the world markers) can draw this frame: the HUD is redirected and drawn as a
/// quad, the game is in its gameplay HUD, and the clean view does not hide everything. Each part
/// adds its own switch, and those that need the controllers add the scheme.
pub(crate) fn world_quads_active(cfg: &HudConfig) -> bool {
    cfg.redirect
        && cfg.quad
        && current_mode() == HudMode::Hud
        && !crate::director::clean_view::hides_everything()
}

/// Whether the floating panel should world-lock this frame: an in-game modal menu (pause /
/// full-screen map), where the render camera still has a valid pose. Narrower than [`current_mode`]'s
/// Movie result -- it excludes the frontend and loading screens (not `E_GAME_RUN`), whose degenerate
/// camera would strand the latched pose. In-game the issue-#7 fix keeps `m_DrawScene` on, so the
/// camera pose is real and the frozen world stays visible behind the panel.
pub fn menu_world_lock() -> bool {
    // SAFETY: GetIUIManager returns the live UI singleton (or null); IsUsingStaticBackGround is a
    // const getter.
    in_gameplay()
        && unsafe {
            GetIUIManager()
                .as_ref()
                .is_some_and(|ui| ui.IsUsingStaticBackGround())
        }
}

/// Aspect ratio (width / height) for the gameplay HUD; `1.0` is square. The effective aspect for
/// the current frame ([`HUD_ASPECT`] or [`MOVIE_ASPECT`], per the [`HudMode`]) is the single source
/// of truth for the HUD's shape: the render-target dimensions, the floating panel, the marker
/// projection ([`compute_panel_vp`]), and the Scaleform viewport all derive from it, so they cannot
/// drift out of sync.
pub(crate) const HUD_ASPECT: f32 = 1.3;

/// Aspect ratio (width / height) for full-screen UI -- movies, loading screens, and menus
/// ([`HudMode::Movie`]); `16:9`. See [`HUD_ASPECT`].
pub(crate) const MOVIE_ASPECT: f32 = 16.0 / 9.0;

/// HUD render-target scale, applied to the geometric mean of the **per-eye** render resolution.
/// The texture's longer axis is `RENDER_SCALE * sqrt(eye_width * eye_height)` pixels; the shorter
/// axis follows from the effective aspect. Ignored when [`RENDER_RESOLUTION`] pins the size
/// outright.
///
/// The geometric mean, rather than the larger axis, keeps the HUD's pixel budget stable as the
/// render target's shape changes: a per-eye target is much taller than it is wide, and keying off
/// the longer axis would inflate the HUD for a shape the HUD does not share.
const RENDER_SCALE: f32 = 1.5;

/// A pin for the HUD render target's longer axis, in pixels, ignoring [`RENDER_SCALE`] and the
/// render resolution entirely. `None` derives the size as described on [`RENDER_SCALE`].
///
/// The HUD is a flat panel viewed at a fixed apparent size, so what it actually needs is enough
/// texels to look crisp at that size -- a property of the headset and the panel's angular size,
/// not of how many pixels the scene happens to be rendered at. A pin makes that budget explicit
/// and immune to render-resolution changes.
const RENDER_RESOLUTION: Option<u32> = None;

/// The effective HUD aspect (width / height) for `mode`: [`HUD_ASPECT`] in gameplay,
/// [`MOVIE_ASPECT`] for full-screen UI.
fn aspect_for(mode: HudMode) -> f32 {
    match mode {
        HudMode::Hud => HUD_ASPECT,
        HudMode::Movie => MOVIE_ASPECT,
    }
}

/// Publish the UI manager's layout from the existing main-movie capture read path.
///
/// # Safety
/// The manager must be the live main-movie UI manager at its game-thread capture boundary,
/// while the UI update/render lock is held.
pub(crate) unsafe fn sample_stage_safe_area(manager: &UIManager) {
    // SAFETY: the capture boundary keeps the manager live under the UI update/render lock.
    let mode = if !in_gameplay() || unsafe { manager.IsUsingStaticBackGround() } {
        HudMode::Movie
    } else {
        HudMode::Hud
    };
    let safe = manager.m_SafeArea;
    let layout = StageSafeArea {
        bounds: [safe.x1, safe.y1, safe.x2, safe.y2],
        percentage: manager.m_SafeAreaPercentage,
        stage: Vec2::new(manager.m_CachedStageWidth, manager.m_CachedStageHeight),
        aspect: effective_aspect(aspect_for(mode)),
    };
    publish_stage_safe_area(layout, Instant::now());
}

/// The effective HUD aspect for the current frame. Used by the marker hook and panel VP, which run
/// outside [`tick`]/[`draw_quad`] and so re-derive the mode.
pub fn current_aspect() -> f32 {
    effective_aspect(aspect_for(current_mode()))
}

fn effective_aspect(requested: f32) -> f32 {
    state::applied_redirect().map_or(requested, |applied| {
        applied.texture.0 as f32 / applied.texture.1 as f32
    })
}

/// The panel's reference width as a fraction of its distance at scale 1.0: 4 m wide at 3 m, the
/// comfortable baseline from issue #14.
const PANEL_WIDTH_PER_DISTANCE: f32 = 4.0 / 3.0;

/// The panel's physical height in meters for the given size knob, distance, and effective aspect.
///
/// The panel width is `scale * PANEL_WIDTH_PER_DISTANCE * distance`, so it grows with distance to
/// keep a constant apparent (angular) size -- moving the panel nearer or farther does not change how
/// big it looks. The height is the width divided by the aspect, so the width (and thus the amount of
/// content that fits horizontally) is invariant to aspect changes: a 16:9 panel is the same width as
/// a 1:1 panel, just shorter.
pub fn panel_height(scale: f32, distance: f32, aspect: f32) -> f32 {
    (scale * PANEL_WIDTH_PER_DISTANCE * distance) / aspect.max(f32::EPSILON)
}

pub(crate) fn redirect_applied() -> bool {
    state::applied_redirect().is_some()
}

/// The per-frame render-thread step: redirects the HUD into our texture while enabled, restores the
/// engine binding while disabled. Runs before the main UI `StartRender` worker is queued, with the
/// UI-deferred lock acquired before the graphics context lock. The `PostDraw` hook provides a
/// fallback when no redirect maintenance ran for the dispatch.
///
/// `window_width`/`window_height` are the swapchain back buffer's dimensions, used only as the
/// cursor mapping's window basis (its fallback for the client rect). Everything sized against the
/// *render* -- the reset proxy and the engine-binding restore -- reads
/// [`crate::stereo::render_size`] instead, because the two diverge once the mod substitutes its own
/// back buffer (`docs/mod/stereo/swapchain-ownership.md`). The HUD texture's own size is neither: it is the
/// mod's choice (see [`hud_target_size`]), as is its aspect.
pub fn tick(device: &Device, window_width: u32, window_height: u32) {
    let mut hud = HUD_STATE.lock();
    let cfg = Config::lock_query(|c| c.hud);
    let render = crate::stereo::render_size().unwrap_or((window_width, window_height));
    if cfg.redirect {
        let aspect = aspect_for(current_mode());
        // Size against one eye's render target: under single-pass the render target is double-wide,
        // and keying off it whole would inflate the HUD by 2x for a target only ever viewed one eye
        // at a time.
        let (eye_width, eye_height) = crate::stereo::per_eye_render_size().unwrap_or(render);
        let (width, height) = hud_target_size(
            RENDER_SCALE,
            RENDER_RESOLUTION,
            aspect,
            eye_width,
            eye_height,
        );
        // The atlas lays the gameplay HUD out inside the stage only, so the bands above and below
        // it are free for cells; menus keep the full layout, and a mode change re-applies the
        // redirect with the right one.
        let stage_safe_area = cfg.atlas.enabled && current_mode() == HudMode::Hud;
        hud.ensure_redirected(device, width, height, render.0, render.1, stage_safe_area);
    } else {
        hud.restore(render.0, render.1);
    }
    // Publish the frame's mouse-mapping geometry for the cursor injection (the `SendMouseEvents`
    // detour): window-client pixels normalize against the window, and rescale to the movie
    // rectangle -- our texture -- once the redirect is applied.
    cursor::set_geometry((window_width, window_height), hud.redirected_size());
}

/// Compute the HUD texture dimensions from the render scale (or the pinned resolution), the aspect
/// (width / height), and the per-eye render size, as described on [`RENDER_SCALE`]; the shorter
/// axis follows from the aspect. Both axes are clamped to at least 1 pixel so a zero-sized
/// back buffer or a degenerate aspect never reaches texture creation.
fn hud_target_size(
    render_scale: f32,
    render_resolution: Option<u32>,
    aspect: f32,
    eye_width: u32,
    eye_height: u32,
) -> (u32, u32) {
    let base = match render_resolution {
        Some(pinned) => pinned.max(1) as f32,
        None => render_scale * (f64::from(eye_width) * f64::from(eye_height)).sqrt() as f32,
    };
    let aspect = aspect.max(f32::EPSILON);
    let (width, height) = if aspect >= 1.0 {
        (base, base / aspect)
    } else {
        (base * aspect, base)
    };
    (
        width.round().clamp(
            1.0,
            crate::config::validate::hud::MAX_TEXTURE_DIMENSION as f32,
        ) as u32,
        height.round().clamp(
            1.0,
            crate::config::validate::hud::MAX_TEXTURE_DIMENSION as f32,
        ) as u32,
    )
}

/// Draw the redirected HUD as a floating quad for `eye` over `target` (the eye's linear back buffer),
/// when both the redirect and the quad are enabled. On eye 0, chooses the panel pose for the current
/// mode (head-following or world-static) and computes the panel's world-space corners (cached for eye
/// 1). Ordinary two-pass rendering crops and clears the HUD texture on every dispatch. Collapse
/// crops only on eye 0, reuses the prepared textures for both eyes, and clears after eye 1. Called
/// from the render-thread post-draw hook, before the back buffer is captured/presented, with the
/// engine context mutex held.
pub fn draw_quad(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    let cfg = Config::lock_query(|c| c.hud);
    let mut hud = HUD_STATE.lock();
    if !cfg.redirect || !cfg.quad || hud.redirected_size().is_none() {
        hud.clear_panel_corners();
        pointer::publish_panel(None);
        return;
    }

    // Compute world-space corners once per frame on eye 0 and cache them. Both eyes then
    // project the same world-space quad through their own per-eye VP, producing correct stereo
    // disparity. Computing corners per-eye instead would cancel the world transform against the
    // per-eye view (VP = Inverse(Transform) · Projection), collapsing the panel to view space
    // (head-locked, zero disparity, appears at infinity).
    let pointer_camera = pointer::rendered_camera();
    if eye == 0
        && let Some((head_pos, head_rotation)) = pointer_camera
            .map(|camera| (camera.head.position, camera.head.orientation))
            .or_else(render_camera_pose)
    {
        let mode = current_mode();
        let aspect = effective_aspect(aspect_for(mode));
        // World-lock the panel while an in-game menu (pause / map) is open, or during a non-gameplay
        // transition in VR (loading screens / fast-travel, issue #27), so it stays put and the player
        // can look around it, reverting to head-follow on close/resume. During a loading transition the
        // panel latches to the head-tracked render pose (`render_camera_pose`), so it appears centered on
        // wherever the head was pointing as the load began and then holds that world spot while the head
        // looks around it.
        let freeze = crate::hooks::camera::vr_loading_view_active() || menu_world_lock();
        let (pos, rot) = hud.update_pose(freeze, head_pos, head_rotation);
        // Dynamic panel distance: ease the panel toward the near field while riding a vehicle
        // (see `depth`). The base (far) distance is the configured one; full-screen UI always
        // reads far.
        let panel_distance = if cfg.depth_shift.enabled {
            hud.depth_distance(mode, cfg.distance)
        } else {
            cfg.distance
        };
        let params_at = |distance: f32| quad::PanelParams {
            pos,
            rot,
            aspect,
            distance,
            panel_height: panel_height(cfg.panel_scale, distance, aspect),
        };
        hud.compute_world_corners(&params_at(panel_distance));
        // The world markers keep the angular size they have on the panel, so they are placed from
        // the panel's angular height (its height over its distance, independent of the distance).
        // SAFETY: the graphics engine is the live singleton on the render thread.
        let graphics_engine =
            unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() };
        atlas::prepare_frame(device, hud.redirected_target(), mode == HudMode::Hud);
        world_markers::prepare_frame(
            context,
            device,
            &world_markers::FrameInputs {
                head: head_pos,
                head_rotation,
                panel_height_over_distance: panel_height(cfg.panel_scale, 1.0, aspect),
                hud: hud.redirected_target(),
                graphics_engine: graphics_engine.as_deref(),
            },
        );
        reticle::prepare_frame(
            device,
            &reticle::FrameInputs {
                head: head_pos,
                head_rotation,
                panel_height_over_distance: panel_height(cfg.panel_scale, 1.0, aspect),
            },
        );
        pointer::publish_panel(pointer_camera.and_then(|camera| {
            Some(pointer::RenderedPanel {
                head: camera.head,
                cockpit_head: camera.cockpit_head,
                scale: camera.scale,
                corners: hud.panel_corners()?,
            })
        }));
        // The panel can draw from the render-camera fallback without the captured tracking sample,
        // which leaves it visible but out of the controller pointer's reach.
        let missing = pointer_camera.is_none();
        if POINTER_BASIS_MISSING.swap(missing, Ordering::Relaxed) != missing && missing {
            tracing::debug!(
                "hud: the panel draws without a pointer basis; the pointer is inactive"
            );
        }
        // The virtual cursor rides the panel at its UV position, lifted toward the camera; its
        // world-space corners are computed here (eye 0) like the panel's so both eyes project the
        // same dot with correct stereo disparity (see `cursor`).
        let cursor_corners = cursor::frame()
            .and_then(|frame| quad::compute_cursor_corners(&params_at(panel_distance), frame));
        hud.set_cursor_corners(cursor_corners);
    } else if eye == 0 {
        // No panel pose this frame: the previous corners are not what the player sees.
        pointer::publish_panel(None);
        world_markers::clear_frame();
        atlas::clear_frame();
        reticle::clear_frame();
    }

    // The world markers are cropped out of this eye's HUD render (from their atlas cells, or cut
    // out of the panel) and the atlas's parts copied out of their cells before the panel draws;
    // the bands that held the cells are then cleared, so the panel shows the stage alone. The
    // markers draw behind the panel (they are anchored in the world, beyond it), and the reticles
    // and the hand and wrist readouts in front of it.
    let collapsed = collapse_active();
    if !collapsed || eye == 0 {
        world_markers::prepare_eye(context, hud.redirected_target());
        atlas::prepare_eye(context, hud.redirected_target());
    }
    if !crate::director::clean_view::hides_everything() {
        world_markers::draw_billboards(context, device, target);
    }
    lost_weapons::draw(context, device, target, eye);
    hud.draw_quad(context, device, target, eye);
    reticle::draw(context, device, target);
    anchored::draw(context, device, target, eye);
    mark_labels::draw(context, device, target, eye);
    grab_hint::draw(context, device, target, eye, hud.panel_corners());
    if !collapsed || eye == 1 {
        hud.clear(context);
    }
}

/// Whether the last panel drawn on eye 0 lacked the pointer's captured camera basis, so the
/// transition is logged once rather than every frame.
static POINTER_BASIS_MISSING: AtomicBool = AtomicBool::new(false);

/// Extract the head's world-space pose `(position, rotation)` from the render camera's world
/// transform, or `None` if the camera is not available.
///
/// The camera's world transform stores its basis vectors in rows (pyxis docs): right (+X), up (+Y),
/// the +Z basis (back), then the translation. A row-major, row-vector matrix's flat array is the
/// column-major array of the equivalent glam matrix, so those rows arrive as `Mat4`'s `x_axis`,
/// `y_axis`, `z_axis`, and `w_axis`. The rotation converts to a quaternion that maps camera-local to
/// world space, so `quat * Vec3::NEG_Z` yields the forward direction.
pub(crate) fn render_camera_pose() -> Option<(Vec3, Quat)> {
    // Prefer the center (un-offset) transform snapshotted in `SetupRenderCamera` before the per-eye
    // parallax offset is applied. By the time `draw_quad` runs on eye 0, `m_TransformF` already
    // carries eye 0's half-IPD offset, which would leak into the cached panel position and double
    // the stereo disparity for eye 1. Fall back to the live transform when the snapshot is absent
    // (e.g. stereo not active, or the hook has not run yet).
    let transform = crate::stereo::STEREO_STATE
        .lock()
        .center_transform
        .or_else(|| unsafe {
            let cm = CameraManager::get()?;
            let cam = cm.m_RenderCamera.as_ref()?;
            Some(cam.m_TransformF)
        })?;

    let transform = Mat4::from(transform);
    Some((
        transform.w_axis.truncate(),
        Quat::from_mat3(&Mat3::from_mat4(transform)),
    ))
}

/// Compute the view-projection and camera matrices for the floating panel's orientation, so that
/// W2S (`Get2DInfo`) projects world points onto the panel's surface rather than the screen plane.
/// Returns `(vp, camera_transform)` in engine format, or `None` if the camera is unavailable.
///
/// The panel VP uses the damped follow rotation for orientation, the head position as the view
/// origin, and a symmetric projection built from the panel's own angular subtense (not the gameplay
/// camera's FOV). This ensures markers are positioned correctly on the floating quad: a POI directly
/// ahead of the camera but off-center from the panel's facing direction appears at the correct
/// position on the panel surface, compensating for the follow-damping lag.
pub fn compute_panel_vp() -> Option<(Matrix4, Matrix4)> {
    // Reuse the pose chosen for the quad this frame (head-following or latched world-static), so
    // markers project onto exactly the panel the quad drew.
    let (pos, rot) = HUD_STATE.lock().panel_pose()?;
    let aspect = current_aspect();
    let panel_scale = Config::lock_query(|c| c.hud.panel_scale);

    let projection = unsafe {
        let cm = CameraManager::get()?;
        let active = cm.m_ActiveCamera.as_ref()?;
        active.m_ProjectionF
    };

    // Build the panel world transform from the pose rotation's basis vectors + the pose position.
    // The engine stores +Z as the third basis (back), so back = quat * Z (not -Z).
    let right = rot * Vec3::X;
    let up = rot * Vec3::Y;
    let back = rot * Vec3::Z;

    let panel_transform = Mat4::from_cols(
        right.extend(0.0),
        up.extend(0.0),
        back.extend(0.0),
        pos.extend(1.0),
    );

    // View = inverse(world transform). VP = P * V (glam column-vector convention).
    // The Matrix4 ↔ Mat4 From impls transpose, so the engine-format result is correct.
    let panel_view = panel_transform.inverse();
    let mut glam_proj = Mat4::from(projection);

    // The projection's field of view must match the panel's own angular subtense as seen from the
    // head -- a horizontal half-angle of `atan(scale * PANEL_WIDTH_PER_DISTANCE / 2)`, independent of
    // distance -- so that a world direction lands where the ray from the head crosses the panel
    // surface. Reusing the gameplay camera's FOV instead misplaces markers by the ratio of the game
    // FOV to the panel subtense: that ratio is ~1 on the flat desktop (the panel effectively fills
    // the screen, so its subtense ≈ the game FOV) but far from 1 in VR, where the panel is a fixed
    // floating surface viewed through a much wider per-eye FOV -- and it swings with every game FOV
    // change (aiming, sniper zoom, vehicles), sliding markers off their world features. Overwrite the
    // FOV terms with the panel's; the panel is centered on the panel-view forward, so the projection
    // is symmetric and any off-center shear is dropped. The depth (`z`) and `w` columns are kept from
    // the game projection, preserving its reverse-Z convention and the behind-camera `w`-sign cull in
    // `Convert3DCoords`. The marker pass also sets `m_CachedViewportRatio = 1 / aspect` so
    // `Convert3DCoords` does not re-apply the device aspect on top of this (see `hooks::ui`). The
    // grapple reticle takes the same projection, so it lands where the head-to-aim ray crosses the
    // panel.
    let cot_x = 2.0 / (panel_scale.max(f32::EPSILON) * PANEL_WIDTH_PER_DISTANCE);
    glam_proj.x_axis.x = cot_x;
    glam_proj.y_axis.y = cot_x * aspect.max(0.0);
    glam_proj.z_axis.x = 0.0;
    glam_proj.z_axis.y = 0.0;

    let glam_vp = glam_proj * panel_view;

    let engine_vp = Matrix4::from(glam_vp);
    let engine_camera = Matrix4::from(panel_transform);

    Some((engine_vp, engine_camera))
}

/// Register the HUD's shutdown cleanup. Call once at init. The cleanup clears the redirect config flag
/// (so the per-frame [`tick`] restores the engine binding on the render thread, as a toggle-off does)
/// and releases the egui preview registrations when the owning renderer is available. This requests
/// render-thread restoration; it does not acknowledge native restoration or establish unload safety.
pub fn install() {
    crate::lifecycle::on_cleanup(|renderer| {
        // Automatically, so that a save during the teardown keeps the redirect the user chose.
        disable_automatically(Switch {
            path: "hud.redirect",
            place: |c| &mut c.hud.redirect,
        });
        if let Some(renderer) = renderer {
            HUD_STATE.lock().release_preview(renderer);
        }
        // The clip handles must be released on the capture (game) thread; the shutdown path lets
        // a few more frames tick before the hooks come down, which drains this request.
        scaleform::request_release_handles();
    });
}

#[cfg(test)]
mod tests {
    use crate::hud::hud_target_size;

    /// The derived size keys off the geometric mean of the per-eye target, so a tall per-eye render
    /// target does not inflate the HUD the way keying off its longer axis did.
    #[test]
    fn derived_size_uses_the_geometric_mean() {
        // 2015x2240 per eye: sqrt(2015 * 2240) = 2124.4, x1.5 = 3186.7.
        let (w, h) = hud_target_size(1.5, None, 1.0, 2015, 2240);
        assert_eq!((w, h), (3187, 3187));
        // Keying off the longer axis would have given 1.5 * 2240 = 3360.
        assert!(w < 3360, "{w} should be below the longer-axis basis");
    }

    /// The aspect shapes the target, and the longer axis carries the budget either way.
    #[test]
    fn aspect_applies_to_the_derived_size() {
        let (w, h) = hud_target_size(1.5, None, 4.0 / 3.0, 2015, 2240);
        assert_eq!(w, 3187);
        assert_eq!(h, 2390);
        let (tall_w, tall_h) = hud_target_size(1.5, None, 0.75, 2015, 2240);
        assert_eq!(tall_h, 3187);
        assert_eq!(tall_w, 2390);
    }

    /// A pinned resolution ignores both the scale and the render resolution entirely.
    #[test]
    fn pinned_resolution_ignores_the_render_resolution() {
        let pinned = hud_target_size(1.5, Some(3072), 1.0, 2015, 2240);
        assert_eq!(pinned, (3072, 3072));
        // Same pin, wildly different render resolution and scale: unchanged.
        assert_eq!(hud_target_size(0.25, Some(3072), 1.0, 640, 480), pinned);
    }

    /// A degenerate pin still yields a usable target rather than a zero-sized texture.
    #[test]
    fn degenerate_inputs_stay_positive() {
        let (w, h) = hud_target_size(1.5, Some(0), 1.0, 2015, 2240);
        assert!(w >= 1 && h >= 1, "{w}x{h}");
        let (w, h) = hud_target_size(0.0, None, 0.0, 1, 1);
        assert!(w >= 1 && h >= 1, "{w}x{h}");
    }
}
