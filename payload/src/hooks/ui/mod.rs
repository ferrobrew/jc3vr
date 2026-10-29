//! UI manager detours: world-to-screen compensation for the floating HUD panel, the aim-depth
//! recording, the capture seam's clip writes, the HUD atlas's moves and the world markers'
//! classification and capture snapshot, (in [`hud`]) the HUD's point-of-interest update, and (in
//! [`mouse`]) the game UI's mouse input.
//!
//! [`get_2d_info`](UIManager::Get2DInfo) is the gameplay marker placement function. When the HUD
//! is redirected and drawn as a floating quad, the VP and camera matrix are replaced with the
//! panel's orientation so that markers project onto the panel's surface rather than the screen
//! plane. See `docs/mod/hud.md`.

mod hud;
mod mouse;

use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use jc3gi::{
    types::math::{Matrix4, Vector2, Vector3},
    ui::{
        scaleform::MovieImpl,
        ui_manager::{ScreenPos, UIManager},
    },
};
use re_utilities::hook_library::HookLibrary;

pub(crate) use crate::hooks::ui::mouse::cancel_pointer_on_game_thread;
use crate::{
    config::Config,
    controllers::{
        overlay::game_reticle::hide_game_reticles, summary::game_hints::hide_game_hints,
    },
    frame_timing::{self, Slot},
    hud::{HUD_STATE, aim, compute_panel_vp, current_aspect},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GET_2D_INFO_BINDER)
        .with_static_binder(&CONVERT_3D_COORDS_DEFAULT_BINDER)
        .with_static_binder(&MOVIE_CAPTURE_BINDER)
        .with_hook_library(hud::hook_library())
        .with_hook_library(mouse::hook_library())
}

/// Marks the start of a game frame for the aim-depth recording: `UpdateGrappleReticle`'s *first*
/// default-VP projection each frame is the game's smoothed aim position; its later calls (the
/// wire-attachment point, the grip-radius sample) are different points and must not be recorded.
/// Called from the game-thread tick.
pub fn begin_frame_aim_recording() {
    AIM_RECORDED_THIS_FRAME.store(false, Ordering::Relaxed);
}

/// Whether the current game frame already recorded its aim depth.
static AIM_RECORDED_THIS_FRAME: AtomicBool = AtomicBool::new(false);

/// The grapple reticle's world-to-screen (the default-VP wrapper whose only callers are in
/// `CHUDUI::UpdateGrappleReticle`). Two jobs: reproject the reticle onto the floating panel (the
/// wrapper's internal VP is not a parameter, so the reticle bypasses the `Get2DInfo` hook's panel
/// reprojection), and record the aim point's depth -- from the first call of the frame only, the
/// game's own smoothed aim position -- into [`crate::hud::aim`].
#[detour(address = UIManager::Convert3DCoordsDefault_ADDRESS)]
fn convert_3d_coords_default(
    this: *mut UIManager,
    world: *const Vector3,
    out_x: *mut f32,
    out_y: *mut f32,
) -> bool {
    let panel_enabled = Config::lock_query(|c| c.hud.redirect && c.hud.quad);

    // SAFETY: `world` is the caller's live aim point; panel_pose only reads cached state.
    if let Some(world) = unsafe { world.as_ref() }
        && let Some((anchor, _)) = HUD_STATE.lock().panel_pose()
    {
        let world = glam::Vec3::new(world.data[0], world.data[1], world.data[2]);
        aim::record(
            (world - anchor)
                .length()
                .clamp(aim::MIN_DEPTH, aim::MAX_DEPTH),
        );
    }

    let aspect = current_aspect();
    // The reticle lands where the head-to-aim ray crosses the panel, like the markers (issue #6).
    let panel = panel_enabled.then(compute_panel_vp).flatten();
    match panel {
        // Project through the panel VP with the aspect retarget, exactly like the Get2DInfo hook,
        // so the grapple reticle lands at the correct spot on the panel surface.
        Some((vp, _camera)) => unsafe {
            let Some(manager) = this.as_mut() else {
                return false;
            };
            let previous = manager.m_CachedViewportRatio;
            manager.m_CachedViewportRatio = 1.0 / aspect.max(f32::EPSILON);
            let ok = manager.Convert3DCoords(world, out_x, out_y, &vp);
            manager.m_CachedViewportRatio = previous;
            ok
        },
        None => CONVERT_3D_COORDS_DEFAULT
            .get()
            .unwrap()
            .call(this, world, out_x, out_y),
    }
}

/// The capture seam: `CUIManager::PreRender` calls `MovieImpl::Capture` right after `Advance`,
/// on the game update thread with the deferred render lock held -- the only point in the frame
/// where every display-tree writer is quiescent. The issue #8 overlay suppression and the
/// controller scheme's reticle and button-hint hiding write here, so the engine's own capture
/// carries everything; the original then runs unchanged. The HUD atlas and the world
/// markers move their clips into atlas cells for this capture only and move them back after it.
/// Movies other than the main UI movie (the render-to-texture instances) pass through untouched.
/// See [`crate::hud::scaleform`] and [`crate::hud::atlas`].
#[detour(address = MovieImpl::CaptureImpl_ADDRESS)]
fn movie_capture(this: *mut MovieImpl, if_changed: bool) -> u64 {
    // SAFETY: the existing main-movie lookup runs at the game-thread capture boundary under the
    // UI update/render lock. Other movies pass through without publishing their layout.
    let is_main_movie = unsafe {
        UIManager::get().is_some_and(|manager| {
            let is_main = std::ptr::eq(manager.m_Movie, this);
            if is_main {
                crate::hud::sample_stage_safe_area(manager);
            }
            is_main
        })
    };
    let timing = frame_timing::scope(Slot::UiCapture);
    if is_main_movie {
        let hud = Config::lock_query(|c| c.hud);
        // SAFETY: this is the capture seam both callees require; `this` is the live main movie.
        unsafe {
            crate::hud::scaleform::apply_overlay_suppression();
            hide_game_reticles();
            hide_game_hints();
            // The atlas moves its parts into their cells for this capture only, and the world
            // markers move into the atlas's marker cells.
            let atlas = crate::hud::atlas::begin_capture(&hud);
            crate::hud::world_markers::on_capture(&hud.world_markers, atlas.as_ref());
            crate::hud::anchored::sample_status(&hud);
        }
    }
    drop(timing);
    let result = MOVIE_CAPTURE.get().unwrap().call(this, if_changed);
    let _timing = frame_timing::scope(Slot::UiCapture);
    // Everything moved for the capture moves back before the next advance, and the world-marker
    // and atlas layouts are keyed by the snapshot this capture produced.
    // SAFETY: `this` is the live movie the capture ran on, at the capture seam.
    if is_main_movie && let Some(movie) = unsafe { this.as_ref() } {
        unsafe {
            crate::hud::world_markers::after_capture(movie);
            crate::hud::atlas::after_capture(movie);
        }
    }
    result
}

/// Replace the VP and camera matrix in `Get2DInfo` with the floating panel's orientation, so
/// world-to-screen projects markers onto the panel surface rather than the screen plane. When the
/// HUD redirect or quad is off, the original caller's VP is passed through unchanged.
//
// The parameter count matches the game's `CUIManager::Get2DInfo` ABI — the detour macro requires an
// exact signature match, so the parameters cannot be bundled into a struct.
#[detour(address = UIManager::Get2DInfo_ADDRESS)]
#[allow(clippy::too_many_arguments)]
fn get_2d_info(
    this: *mut UIManager,
    world: *const Vector3,
    vp_orig: *const Matrix4,
    camera_orig: *const Matrix4,
    a5: f32,
    out_x: *mut f32,
    out_y: *mut f32,
    out_pos: *mut ScreenPos,
    margin: f32,
    a10: bool,
    offset: Vector2,
) {
    let (panel_enabled, world_markers) =
        Config::lock_query(|c| (c.hud.redirect && c.hud.quad, c.hud.world_markers));
    let aspect = current_aspect();
    let panel = panel_enabled.then(compute_panel_vp).flatten();
    let (vp, camera) = panel
        .as_ref()
        .map(|(v, c)| (v as *const Matrix4, c as *const Matrix4))
        .unwrap_or((vp_orig, camera_orig));

    // For the panel pass, retarget `Convert3DCoords`' aspect correction to the panel: it reads
    // `m_CachedViewportRatio` (the window height/width, refreshed every frame), which would skew
    // markers off the panel on top of the already-re-aspected panel VP. Set it to the panel's
    // `height / width` (= `1 / aspect`) for the call and restore it afterwards so other
    // world-to-screen consumers in the same frame are unaffected.
    let restore_ratio = panel
        .is_some()
        .then(|| unsafe {
            this.as_mut().map(|manager| {
                let previous = manager.m_CachedViewportRatio;
                manager.m_CachedViewportRatio = 1.0 / aspect.max(f32::EPSILON);
                previous
            })
        })
        .flatten();

    GET_2D_INFO.get().unwrap().call(
        this, world, vp, camera, a5, out_x, out_y, out_pos, margin, a10, offset,
    );

    if let Some(previous) = restore_ratio {
        unsafe {
            if let Some(manager) = this.as_mut() {
                manager.m_CachedViewportRatio = previous;
            }
        }
    }

    // The world markers classify the points of interest's projections, and may move one the panel
    // put off screen but the headset can see back on screen.
    // SAFETY: the original call populated the out pointers; the camera is the caller's own.
    unsafe {
        if let (Some(manager), Some(world), Some(camera), Some(out_x), Some(out_y), Some(out_pos)) = (
            this.as_ref(),
            world.as_ref(),
            camera_orig.as_ref(),
            out_x.as_mut(),
            out_y.as_mut(),
            out_pos.as_mut(),
        ) {
            crate::hud::world_markers::classify(
                crate::hud::world_markers::Projection {
                    manager,
                    world: glam::Vec3::from(world.data),
                    camera,
                    margin,
                    out_x,
                    out_y,
                    out_pos,
                },
                &world_markers,
            );
        }
    }
}
