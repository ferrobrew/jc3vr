//! UI manager detours: world-to-screen compensation for the floating HUD panel.
//!
//! [`get_2d_info`](UIManager::Get2DInfo) is the gameplay marker placement function. When the HUD
//! is redirected and drawn as a floating quad, the VP and camera matrix are replaced with the
//! panel's orientation so that markers project onto the panel's surface rather than the screen
//! plane. See `docs/mod/hud.md`.

use detours_macro::detour;
use jc3gi::{
    graphics_engine::graphics_engine::HContext_t,
    types::math::{Matrix4, Vector2, Vector3},
    ui::{
        overlay_ui::OverlayUI,
        scaleform::{MouseEvent, MovieImpl},
        ui_manager::{ScreenPos, UIManager},
    },
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    hud::{HUD_STATE, compute_panel_vp, current_aspect, cursor, pointer, split::roots},
};

/// Cancel input on the game thread before shutdown removes the UI hooks.
pub(crate) fn cancel_pointer_on_game_thread() {
    crate::controllers::map::cancel();
    pointer::clear_controller();
    pointer::reset_route();
    cursor::set_frame(None);
    // SAFETY: shutdown calls this on the game thread while engine singletons are live.
    unsafe {
        let movie = UIManager::get().and_then(|manager| manager.m_Movie.as_mut());
        release_pointer(movie);
    }
}

fn release_pointer(movie: Option<&mut MovieImpl>) {
    let live = movie
        .as_ref()
        .map(|movie| std::ptr::from_ref::<MovieImpl>(*movie) as usize);
    let release = SYNTHETIC_MOUSE.lock().cancel(live);
    if let (Some(movie), Some((x, y))) = (movie, release) {
        // SAFETY: the caller supplies only the current live movie, on an engine UI thread.
        unsafe { movie.NotifyMouseState(x, y, 0, 0) };
    }
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GET_2D_INFO_BINDER)
        .with_static_binder(&UI_RENDER_BINDER)
        .with_static_binder(&CONVERT_3D_COORDS_DEFAULT_BINDER)
        .with_static_binder(&MOVIE_CAPTURE_BINDER)
        .with_static_binder(&SEND_MOUSE_EVENTS_BINDER)
        .with_static_binder(&GET_MOVIE_SPACE_MOUSE_CURSOR_BINDER)
}

/// Marks the start of a game frame for the aim-depth recording: `UpdateGrappleReticle`'s *first*
/// default-VP projection each frame is the game's smoothed aim position; its later calls (the
/// wire-attachment point, the grip-radius sample) are different points and must not be recorded.
/// Called from the game-thread tick.
pub fn begin_frame_aim_recording() {
    AIM_RECORDED_THIS_FRAME.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the current game frame already recorded its aim depth.
static AIM_RECORDED_THIS_FRAME: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The grapple reticle's world-to-screen (the default-VP wrapper whose only callers are in
/// `CHUDUI::UpdateGrappleReticle`). Two jobs: reproject the reticle onto the floating panel (the
/// wrapper's internal VP is not a parameter, so the reticle bypasses the `Get2DInfo` hook's panel
/// reprojection), and record the aim point's depth -- from the first call of the frame only, the
/// game's own smoothed aim position -- for the center layer's aim-driven distance.
#[detour(address = UIManager::Convert3DCoordsDefault_ADDRESS)]
fn convert_3d_coords_default(
    this: *mut UIManager,
    world: *const Vector3,
    out_x: *mut f32,
    out_y: *mut f32,
) -> bool {
    let (panel_enabled, record_aim, max_depth, reticle_align) = Config::lock_query(|c| {
        (
            c.hud.redirect && c.hud.quad,
            c.hud.center_depth_from_aim,
            c.hud.marker_max_depth,
            c.hud.reticle_align,
        )
    });

    // SAFETY: `world` is the caller's live aim point; panel_pose only reads cached state.
    if record_aim
        && let Some(world) = unsafe { world.as_ref() }
        && let Some((anchor, _)) = HUD_STATE.lock().panel_pose()
    {
        let world = glam::Vec3::new(world.data[0], world.data[1], world.data[2]);
        crate::hud::aim::record((world - anchor).length().clamp(0.5, max_depth.max(0.5)));
    }

    let aspect = current_aspect();
    // The reticle's projection is a tunable A/B (issue #6): the panel subtense (default) or the game's
    // own projection. Markers stay on the panel subtense.
    let symmetric = reticle_align == crate::hud::ReticleAlign::PanelSubtense;
    let panel = panel_enabled.then(|| compute_panel_vp(symmetric)).flatten();
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
/// where every display-tree writer is quiescent. The render-root partition maintains itself here
/// (build, per-frame reconcile against pool churn, teardown), and the issue #8 overlay
/// suppression writes here, so the engine's own capture carries everything; the original then
/// runs unchanged. Movies other than the main UI movie (the render-to-texture instances) pass
/// through untouched. See [`crate::hud::split::roots`] and [`crate::hud::scaleform`].
#[detour(address = MovieImpl::CaptureImpl_ADDRESS)]
fn movie_capture(this: *mut MovieImpl, if_changed: bool) -> u64 {
    // SAFETY: the UI manager is a live singleton past startup; the pointer comparison does not
    // dereference `this`.
    let is_main_movie =
        unsafe { UIManager::get().is_some_and(|manager| std::ptr::eq(manager.m_Movie, this)) };
    if is_main_movie {
        let (split_enabled, suppress_overlays) = Config::lock_query(|c| {
            (
                c.hud.redirect && c.hud.quad && c.hud.split,
                c.hud.suppress_overlays,
            )
        });
        // The partition persists across menus and pauses: the render detour only composites
        // per-layer in gameplay, and the main root (where menus live) renders normally either
        // way. Tearing down per mode change would churn the render tree with hundreds of
        // structural ops per pause, for no benefit.
        let split_active = split_enabled
            && crate::hud::scaleform::handles_hud_fresh()
            && crate::hud::split_layers_ready();
        // SAFETY: this is the capture seam both callees require; `this` is the live main movie.
        unsafe {
            crate::hud::scaleform::apply_overlay_suppression(suppress_overlays);
            if let Some(movie) = this.as_mut() {
                roots::on_capture(movie, split_active);
                log_pipeline_lag(movie, split_active);
            }
        }
    }
    MOVIE_CAPTURE.get().unwrap().call(this, if_changed)
}

/// Log the snapshot pipeline's produced-vs-displayed gap every few seconds: the displayed UI
/// trailing the update thread by a growing number of captures means the render side is not
/// consuming them (the "elements update late" symptom); a steady small gap acquits the snapshot
/// pipeline and points at the update side instead.
fn log_pipeline_lag(movie: &MovieImpl, split_active: bool) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST_LOG: parking_lot::Mutex<Option<std::time::Instant>> = parking_lot::Mutex::new(None);
    static LAST_ACTIVE: AtomicU64 = AtomicU64::new(0);
    let due = {
        let mut last = LAST_LOG.lock();
        let due = last.is_none_or(|t| t.elapsed().as_secs_f32() >= 5.0);
        if due {
            *last = Some(std::time::Instant::now());
        }
        due
    };
    if !due {
        return;
    }
    let ids = movie.RenderContext.SnapshotFrameIds;
    let produced_in_window = ids[0].wrapping_sub(LAST_ACTIVE.swap(ids[0], Ordering::Relaxed));
    tracing::debug!(
        "scaleform pipeline: active {} displaying {} (lag {}), {} captures in window, \
         partition {}",
        ids[0],
        ids[2],
        ids[0].saturating_sub(ids[2]),
        produced_in_window,
        if split_active { "active" } else { "inactive" },
    );
}

/// Render the partitioned HUD (each render root into its own layer texture, full rate) while the
/// partition is live; otherwise pass through. Runs on the UI render worker (kicked by
/// `StartRender`). See [`crate::hud::split::roots`].
#[detour(address = UIManager::RenderImpl_ADDRESS)]
fn ui_render(this: *mut UIManager, context: *mut HContext_t) {
    let original = UI_RENDER.get().unwrap();
    if let Some(views) = crate::hud::split_inputs() {
        // SAFETY: called from the detour with the detour's own arguments, on the UI render
        // worker.
        if unsafe { roots::render_partitioned(this, &views) } {
            return;
        }
    }
    original.call(this, context);
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
    let (panel_enabled, record_depths, max_depth, marker_radius) = Config::lock_query(|c| {
        (
            c.hud.redirect && c.hud.quad,
            c.hud.redirect && c.hud.quad && c.hud.marker_warp,
            c.hud.marker_max_depth,
            c.hud.marker_radius,
        )
    });
    let aspect = current_aspect();
    // Markers always use the panel subtense; only the reticle exposes the game-projection A/B.
    let panel = panel_enabled.then(|| compute_panel_vp(true)).flatten();
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

    // Record on-screen markers for the marker-layer depth warp: where the marker landed on the
    // panel texture (stage coordinates over the cached stage size) and how far its world point is
    // from the panel anchor. Edge-clamped markers are directional indicators, not world points,
    // and stay at the layer's base depth.
    // SAFETY: the original call populated the out pointers; `this` and `world` are the caller's
    // live arguments.
    if record_depths && panel.is_some() {
        unsafe {
            if let (Some(manager), Some(world), Some(&pos)) =
                (this.as_ref(), world.as_ref(), out_pos.as_ref())
                && pos == ScreenPos::SCREEN_POS_ONSCREEN
                && manager.m_CachedStageWidth > 0.0
                && manager.m_CachedStageHeight > 0.0
                && let Some((anchor, _)) = HUD_STATE.lock().panel_pose()
            {
                let world = glam::Vec3::new(world.data[0], world.data[1], world.data[2]);
                let depth = (world - anchor).length().clamp(0.5, max_depth.max(0.5));
                crate::hud::markers::record(crate::hud::markers::MarkerDepth {
                    u: *out_x / manager.m_CachedStageWidth,
                    v: *out_y / manager.m_CachedStageHeight,
                    depth,
                    radius: marker_radius,
                });
            }
        }
    }
}

#[derive(Default)]
struct SyntheticMouse {
    movie: Option<usize>,
    position: (f32, f32),
    buttons: u32,
    pointer: Option<(pointer::RoutedPointer, std::time::Instant)>,
}

fn pointer_visible(controller: bool, desktop_visible: bool, gamepad: bool) -> bool {
    controller || (desktop_visible && !gamepad)
}

impl SyntheticMouse {
    pub(crate) fn resolve_delivery(
        &self,
        live: usize,
        mut routed: pointer::RoutedPointer,
        desktop_uv: (f32, f32),
    ) -> (pointer::RoutedPointer, Option<(f32, f32)>) {
        let cancellation = routed.controller && routed.uv.is_none();
        let previous = self
            .pointer
            .filter(|(pointer, _)| self.movie == Some(live) && pointer.controller);
        if cancellation {
            routed.uv = previous.and_then(|(pointer, _)| pointer.uv);
            routed.buttons = 0;
        }
        routed.uv = Some(routed.uv.unwrap_or(desktop_uv));
        (
            routed,
            (cancellation && previous.is_some()).then_some(self.position),
        )
    }

    fn selected_uv(&self, live: usize, now: std::time::Instant) -> Option<(f32, f32)> {
        self.pointer
            .filter(|(_, time)| {
                self.movie == Some(live)
                    && now.saturating_duration_since(*time)
                        <= crate::controllers::frame::MAX_FRAME_AGE
            })
            .and_then(|(pointer, _)| pointer.controller.then_some(pointer.uv).flatten())
    }

    fn cancel(&mut self, live: Option<usize>) -> Option<(f32, f32)> {
        let release = (self.buttons != 0 && self.movie.is_some() && self.movie == live)
            .then_some(self.position);
        *self = Self::default();
        release
    }
}

static SYNTHETIC_MOUSE: Mutex<SyntheticMouse> = Mutex::new(SyntheticMouse {
    movie: None,
    position: (0.0, 0.0),
    buttons: 0,
    pointer: None,
});

#[cfg(test)]
mod pointer_tests {
    use super::*;

    fn controller(uv: (f32, f32), buttons: u32) -> pointer::ControllerPointer {
        pointer::ControllerPointer {
            hand: crate::controllers::frame::Hand::Right,
            uv,
            buttons,
            owns_ui: true,
        }
    }

    fn deliver(
        router: &mut pointer::RouterState,
        state: &mut SyntheticMouse,
        controller: Option<pointer::ControllerPointer>,
        desktop_uv: (f32, f32),
        buttons: u32,
    ) -> (pointer::RoutedPointer, (f32, f32)) {
        let routed = router.route(controller, buttons);
        let (routed, position) = state.resolve_delivery(1, routed, desktop_uv);
        let uv = routed.uv.unwrap();
        let position = position.unwrap_or((uv.0 * 1000.0, uv.1 * 1000.0));
        *state = SyntheticMouse {
            movie: Some(1),
            position,
            buttons: routed.buttons,
            pointer: Some((routed, std::time::Instant::now())),
        };
        (routed, position)
    }

    #[test]
    fn desktop_release_is_delivered_before_controller_hover() {
        let mut router = pointer::RouterState::new();
        let mut state = SyntheticMouse::default();
        let hover = Some(controller((0.9, 0.8), 0));
        let (press, position) = deliver(&mut router, &mut state, None, (0.1, 0.2), 1);
        assert_eq!(
            (press.uv, position, press.buttons),
            (Some((0.1, 0.2)), (100.0, 200.0), 1)
        );
        let (release, position) = deliver(&mut router, &mut state, hover, (0.2, 0.3), 0);
        assert!(!release.controller);
        assert_eq!(
            (release.uv, position, release.buttons),
            (Some((0.2, 0.3)), (200.0, 300.0), 0)
        );
        assert_eq!(state.selected_uv(1, std::time::Instant::now()), None);
        let (hover, position) = deliver(&mut router, &mut state, hover, (0.2, 0.3), 0);
        assert_eq!(
            (hover.uv, position, hover.buttons),
            (Some((0.9, 0.8)), (900.0, 800.0), 0)
        );
    }

    #[test]
    fn controller_loss_releases_at_last_delivered_position_before_desktop() {
        let mut router = pointer::RouterState::new();
        let mut state = SyntheticMouse::default();
        let desktop = (0.1, 0.2);
        deliver(
            &mut router,
            &mut state,
            Some(controller((0.8, 0.9), 0)),
            desktop,
            0,
        );
        let (press, _) = deliver(
            &mut router,
            &mut state,
            Some(controller((0.8, 0.9), 1)),
            desktop,
            0,
        );
        assert_eq!((press.uv, press.buttons), (Some((0.8, 0.9)), 1));
        // A cancellation preserves delivered pixels even if the live viewport mapping changes.
        state.position = (731.0, 842.0);
        let (release, position) = deliver(&mut router, &mut state, None, desktop, 0);
        assert!(release.controller);
        assert_eq!(
            (release.uv, position, release.buttons),
            (Some((0.8, 0.9)), (731.0, 842.0), 0)
        );
        assert_eq!(
            state.selected_uv(1, std::time::Instant::now()),
            Some((0.8, 0.9))
        );
        let (fallback, position) = deliver(&mut router, &mut state, None, desktop, 0);
        assert_eq!(
            (fallback.uv, position, fallback.buttons),
            (Some(desktop), (100.0, 200.0), 0)
        );
        assert!(!fallback.controller);
    }

    #[test]
    fn controller_release_uses_current_outside_hit_not_previous_position() {
        let mut router = pointer::RouterState::new();
        let mut state = SyntheticMouse::default();
        let desktop = (0.1, 0.2);
        deliver(
            &mut router,
            &mut state,
            Some(controller((0.8, 0.9), 0)),
            desktop,
            0,
        );
        let (press, position) = deliver(
            &mut router,
            &mut state,
            Some(controller((0.8, 0.9), 1)),
            desktop,
            0,
        );
        assert_eq!(
            (press.uv, position, press.buttons),
            (Some((0.8, 0.9)), (800.0, 900.0), 1)
        );
        let (release, position) = deliver(
            &mut router,
            &mut state,
            Some(controller((1.0, 0.7), 0)),
            desktop,
            0,
        );
        assert_eq!(
            (release.uv, position, release.buttons),
            (Some((1.0, 0.7)), (1000.0, 700.0), 0)
        );
        assert_eq!(
            state.selected_uv(1, std::time::Instant::now()),
            Some((1.0, 0.7))
        );
    }

    #[test]
    fn controller_feedback_is_independent_of_desktop_policy() {
        for desktop in [false, true] {
            for gamepad in [false, true] {
                assert!(pointer_visible(true, desktop, gamepad));
                assert_eq!(
                    pointer_visible(false, desktop, gamepad),
                    desktop && !gamepad
                );
            }
        }
    }

    #[test]
    fn selected_map_uv_matches_only_fresh_live_controller_delivery() {
        let now = std::time::Instant::now();
        let mut state = SyntheticMouse {
            movie: Some(1),
            position: (12.0, 24.0),
            buttons: 1,
            pointer: Some((
                pointer::RoutedPointer {
                    controller: true,
                    uv: Some((0.2, 0.8)),
                    buttons: 1,
                },
                now,
            )),
        };
        assert_eq!(state.selected_uv(1, now), Some((0.2, 0.8)));
        assert_eq!(state.selected_uv(2, now), None);
        assert_eq!(
            state.selected_uv(
                1,
                now + crate::controllers::frame::MAX_FRAME_AGE
                    + std::time::Duration::from_millis(1)
            ),
            None
        );
        state.pointer.as_mut().unwrap().0.controller = false;
        assert_eq!(state.selected_uv(1, now), None);
        state.pointer.as_mut().unwrap().0.controller = true;
        state.cancel(None);
        assert_eq!(state.selected_uv(1, now), None);
        assert_eq!(state.buttons, 0);
    }

    #[test]
    fn cancellation_releases_only_the_live_movie_once() {
        for live in [Some(1), Some(2), None] {
            let mut state = SyntheticMouse {
                movie: Some(1),
                position: (12.0, 24.0),
                buttons: 1,
                pointer: None,
            };
            let mut events = vec![1];
            if state.cancel(live).is_some() {
                events.push(0);
            }
            assert_eq!(events, if live == Some(1) { vec![1, 0] } else { vec![1] });
            assert!(state.cancel(live).is_none());
        }
    }
}

/// Replace the game's mouse-to-UI coordinate mapping while the HUD redirect is active.
///
/// The original converts window-client pixels to movie-viewport pixels by subtracting the
/// centering offset `(m_CachedViewport - m_MovieScale) / 2` -- but the redirect points both of
/// those at our offscreen texture, so window coordinates no longer relate to either and the
/// original's mapping lands nowhere near the pointer. It also only emits a move event on frames
/// where the DirectInput mouse reported a delta, and reads clicks out of the steering action map.
///
/// Instead: normalize the window-client position ([`UIManager::m_MouseX`], written by `WndProc` on
/// every `WM_MOUSEMOVE`) against the window size, rescale to the movie rectangle (= our texture),
/// and hand Scaleform the whole mouse state -- position plus the `WndProc`-tracked button bitmask
/// -- via `NotifyMouseState`, which diffs against the previous state on the next `Advance` to
/// synthesize hover/press/release. The wheel still needs an explicit event. The game's own
/// `MCI_cursor` sprite is parked offscreen (it would otherwise be drawn into the panel texture
/// alongside our own cursor), and the panel cursor's position and visibility are published for the
/// render side (see [`crate::hud::cursor`]).
///
/// Like the original, it runs both from `WndProc` (via `SetMousePos`) and once per frame from
/// `CUIManager::PreUpdate`, so the state stays fresh even while the physical mouse is still.
#[detour(address = UIManager::SendMouseEvents_ADDRESS)]
fn send_mouse_events(this: *mut UIManager, steering: *mut std::ffi::c_void) -> bool {
    let original = SEND_MOUSE_EVENTS.get().unwrap();

    let active = !crate::is_shutting_down()
        && Config::lock_query(|c| c.hud.redirect && c.hud.cursor.enabled);
    // While egui captures input the debug panel owns the mouse; leave the game's own path in
    // place (its input manager is disabled anyway) and hide the panel cursor.
    let egui_captured = crate::egui_impl::EguiState::get()
        .as_ref()
        .is_some_and(|s| s.is_input_captured());
    // The mapping geometry is published by the render-thread HUD tick only once the redirect is
    // applied; until then the original's coordinate spaces are still intact.
    let geometry = cursor::geometry();
    if !active || egui_captured || geometry.is_none() {
        cursor::set_frame(None);
        // Clearing the source before restoring it prevents a held controller trigger from being
        // resurrected if the redirect, geometry, or egui ownership returns later.
        pointer::clear_controller();
        pointer::reset_route();
        // SAFETY: only the movie supplied by the current live manager is dereferenced.
        unsafe {
            release_pointer(this.as_mut().and_then(|manager| manager.m_Movie.as_mut()));
        }
        return original.call(this, steering);
    }
    let routed = crate::controllers::map::prepared_routed().unwrap_or_else(|| {
        let controller = pointer::controller_output().filter(|p| p.owns_ui);
        pointer::route_game_pointer(controller)
    });
    let ((window_w, window_h), (movie_w, movie_h)) = geometry.unwrap();

    // SAFETY: `this` is the live UI singleton (the engine's own callers pass it); the movie
    // pointer is checked before use.
    let Some(manager) = (unsafe { this.as_mut() }) else {
        return original.call(this, steering);
    };
    let Some(movie) = (unsafe { manager.m_Movie.as_mut() }) else {
        pointer::clear_controller();
        pointer::reset_route();
        SYNTHETIC_MOUSE.lock().cancel(None);
        cursor::set_frame(None);
        return false;
    };
    let movie_identity = std::ptr::from_ref(movie) as usize;
    if SYNTHETIC_MOUSE
        .lock()
        .movie
        .is_some_and(|id| id != movie_identity)
    {
        SYNTHETIC_MOUSE.lock().cancel(None);
        pointer::clear_controller();
        pointer::reset_route();
        cursor::set_frame(None);
        unsafe { movie.NotifyMouseState(-1000.0, -1000.0, 0, 0) };
        return true;
    }

    // Mirror the original's gamepad handling: park the Scaleform mouse out of the movie so
    // gamepad-driven menu focus is not fought by a stale hover.
    let desktop_capture = {
        let delivered = SYNTHETIC_MOUSE.lock();
        delivered.movie == Some(movie_identity)
            && delivered.buttons != 0
            && delivered
                .pointer
                .is_some_and(|(pointer, _)| !pointer.controller)
    };
    if manager.m_IsUsingGamepad && !routed.controller && !desktop_capture {
        // SAFETY: NotifyMouseState is the movie's own embedding API, called from the same
        // contexts the engine calls HandleEvent from.
        unsafe { movie.NotifyMouseState(-1000.0, -1000.0, 0, 0) };
        SYNTHETIC_MOUSE.lock().cancel(None);
        cursor::set_frame(None);
        return true;
    }

    let desktop_uv = (
        (manager.m_MouseX as f32 / window_w as f32).clamp(0.0, 1.0),
        (manager.m_MouseY as f32 / window_h as f32).clamp(0.0, 1.0),
    );
    let (routed, release_position) =
        SYNTHETIC_MOUSE
            .lock()
            .resolve_delivery(movie_identity, routed, desktop_uv);
    let (u, v) = routed.uv.expect("delivery resolves pointer coordinates");

    // Map through the movie's LIVE stage-to-viewport matrix rather than assuming any pixel
    // scale: HandleEvent/NotifyMouseState invert `MovieImpl.ViewportMatrix` to produce the stage
    // point they hit-test, and that copy of the matrix can be stale relative to the render
    // root's (observed in practice: the render root carries the texture-shaped matrix while the
    // movie still holds the load-time stage-identity one, so injected texture pixels were
    // consumed as stage pixels -- a uniform ~0.73 compression toward the top-left). Computing
    // the desired stage point (the UI fills the texture with the stage) and running it FORWARD
    // through the live matrix is exact no matter which matrix the movie currently holds.
    // The matrix maps stage twips (20/px) to view-rectangle pixels: `px = s * twips + t`.
    let stage_w = manager.m_CachedStageWidth;
    let stage_h = manager.m_CachedStageHeight;
    let m = movie.ViewportMatrix;
    let (x, y) = if stage_w > 0.0 && stage_h > 0.0 && m[0] != 0.0 && m[5] != 0.0 {
        let stage_x_twips = u * stage_w * 20.0;
        let stage_y_twips = v * stage_h * 20.0;
        (m[0] * stage_x_twips + m[3], m[5] * stage_y_twips + m[7])
    } else {
        // Degenerate stage or matrix (movie still initializing): fall back to texture pixels.
        (u * movie_w as f32, v * movie_h as f32)
    };

    let (x, y) = release_position.unwrap_or((x, y));

    // SAFETY: as above; the wheel event matches the layout the engine itself builds for
    // HandleEvent, and the overlay call no-ops unless the overlay is active.
    unsafe {
        movie.NotifyMouseState(x, y, routed.buttons, 0);
        *SYNTHETIC_MOUSE.lock() = SyntheticMouse {
            movie: Some(movie_identity),
            position: (x, y),
            buttons: routed.buttons,
            pointer: Some((routed, std::time::Instant::now())),
        };

        let wheel_lines = cursor::take_wheel_lines();
        if wheel_lines != 0.0 {
            let mut event: MouseEvent = std::mem::zeroed();
            event.Type = MouseEvent::TYPE_MOUSE_WHEEL;
            event.x = x;
            event.y = y;
            event.ScrollDelta = wheel_lines;
            movie.HandleEvent(&event);
        }

        // Park the game's in-movie cursor sprite: with the original bypassed nothing repositions
        // it, and its last position would ghost inside the panel texture under our own cursor.
        if let Some(overlay) = OverlayUI::get() {
            overlay.SetMouseCursorPosition(-10_000.0, -10_000.0);
        }
    }

    // Controller feedback remains visible when gamepad policy hides the desktop cursor.
    // SAFETY: reads a field of the live overlay singleton.
    let visible =
        unsafe { OverlayUI::get() }.is_some_and(|overlay| overlay.m_MouseCursorShowRefCount > 0);
    cursor::set_frame(
        pointer_visible(routed.controller, visible, manager.m_IsUsingGamepad)
            .then_some(cursor::CursorFrame { u, v }),
    );
    true
}

/// The map's mouse-to-stage conversion: `CCommMapUI::OnManageInput` -- the function's only
/// caller -- feeds `GetMousePos` window-client pixels through it to get the stage-space cursor
/// position it uses for icon picking, click selection, drag panning, and zoom-to-cursor. The
/// original maps `(pos - m_MouseDelta) * m_MouseScaleFac`, and those fields are written only by
/// `ComputeMovieSizeOnViewSize` -- which the redirect bypasses, leaving them describing the
/// window-shaped movie rectangle (X happens to stay correct because its delta is zero and the
/// scale factor cancels; Y is offset by the stale half-letterbox). Generic Scaleform widgets are
/// unaffected -- they hit-test through the injected `NotifyMouseState` path -- so the map was the
/// one UI screen left broken. Replace the mapping with the same client-rect normalization the
/// injection uses, so the map's cursor and the panel dot agree by construction.
#[detour(address = UIManager::GetMovieSpaceMouseCursor_ADDRESS)]
fn get_movie_space_mouse_cursor(
    this: *const UIManager,
    viewport_x: f32,
    viewport_y: f32,
    out: *mut jc3gi::types::math::Vector2,
) {
    let original = GET_MOVIE_SPACE_MOUSE_CURSOR.get().unwrap();
    let geometry = Config::lock_query(|c| c.hud.redirect)
        .then(cursor::geometry)
        .flatten();
    // SAFETY: `this` is the live UI singleton (the map passes it); `out` is the caller's
    // out-pointer, written exactly once.
    unsafe {
        let (Some((window, _)), Some(manager)) = (geometry, this.as_ref()) else {
            return original.call(this, viewport_x, viewport_y, out);
        };
        let (stage_w, stage_h) = (manager.m_CachedStageWidth, manager.m_CachedStageHeight);
        if stage_w <= 0.0 || stage_h <= 0.0 {
            return original.call(this, viewport_x, viewport_y, out);
        }
        // Native map input runs before `SendMouseEvents` in `UIManager::PreUpdate`. Prefer the
        // controller owner snapshot directly so map hit-testing sees this tick's coherent UV rather
        // than the previous delivered movie event. This read does not advance the router; the mouse
        // detour remains the sole owner of source transitions and button delivery.
        let selected = crate::controllers::map::prepared_uv().map(|uv| (uv.x, uv.y));
        let selected = selected.or_else(|| {
            SYNTHETIC_MOUSE
                .lock()
                .selected_uv(manager.m_Movie as usize, std::time::Instant::now())
        });
        let (u, v) = selected.unwrap_or_else(|| {
            (
                (viewport_x / window.0 as f32).clamp(0.0, 1.0),
                (viewport_y / window.1 as f32).clamp(0.0, 1.0),
            )
        });
        if let Some(out) = out.as_mut() {
            out.data = [u * stage_w, v * stage_h];
        }
    }
}
