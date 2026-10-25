//! The game UI's mouse input while the HUD is redirected: one delivery per `SendMouseEvents` call,
//! from whichever source the pointer router selects, and one cancellation path for every way a
//! delivery can stop.
//!
//! `UIManager::PreUpdate` is the frame's UI input boundary. Its detour peeks the route once, before
//! the input dispatch, so the native map and the Scaleform delivery see the same sample; the route
//! is committed only when `SendMouseEvents` actually delivers it (see
//! [`crate::hud::pointer::PendingRoute`]).
//!
//! Every cancellation (the redirect or cursor turning off, egui taking input, a missing or replaced
//! movie, shutdown) goes through [`cancel_pointer`], which releases a held button off the movie at
//! the engine's own gamepad park position. Scaleform treats a release outside the pressed object as
//! no click, so a cancelled press never activates the control under it.

use std::{ffi::c_void, ptr};

use detours_macro::detour;
use jc3gi::{
    types::math::Vector2,
    ui::{
        overlay_ui::OverlayUI,
        scaleform::{MouseEvent, MovieImpl},
        ui_manager::UIManager,
    },
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::map,
    egui_impl::debug_ui_capture,
    hud::{
        cursor,
        pointer::{self, PendingRoute, RoutedPointer},
    },
};

/// Cancel input on the game thread before ownership changes or shutdown removes the UI hooks.
pub(crate) fn cancel_pointer_on_game_thread() {
    map::cancel();
    // SAFETY: callers run on the game thread while engine singletons are live.
    unsafe {
        let movie = UIManager::get().and_then(|manager| manager.m_Movie.as_mut());
        cancel_pointer_for_ownership(movie);
    }
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&PRE_UPDATE_BINDER)
        .with_static_binder(&SEND_MOUSE_EVENTS_BINDER)
        .with_static_binder(&GET_MOVIE_SPACE_MOUSE_CURSOR_BINDER)
}

/// The frame's UI input boundary. The route and the native map's controller sample are prepared
/// before the original dispatches input, and discarded after it returns, so a frame on which the
/// engine skips the dispatch (no input-priority UI) consumes no transition.
#[detour(address = UIManager::PreUpdate_ADDRESS)]
fn pre_update(this: *mut UIManager, dt: f32) {
    let pending = mouse_route_active().then(|| {
        let controller = pointer::selected_controller();
        (controller, pointer::peek_route(controller))
    });
    map::prepare_dispatch(
        pending.and_then(|(controller, _)| controller),
        pending.map(|(_, route)| route.routed),
    );
    *PREPARED_ROUTE.lock() = pending.map(|(_, route)| route);
    PRE_UPDATE.get().unwrap().call(this, dt);
    *PREPARED_ROUTE.lock() = None;
    map::finish_dispatch();
}

/// Replace the game's mouse-to-UI coordinate mapping while the HUD redirect is active.
///
/// The original converts window-client pixels to movie-viewport pixels by subtracting the
/// centering offset `(m_CachedViewport - m_MovieScale) / 2` -- but the redirect points both of
/// those at our offscreen texture, so window coordinates no longer relate to either and the
/// original's mapping lands nowhere near the pointer. It also only emits a move event on frames
/// where the DirectInput mouse reported a delta, and reads clicks out of the steering action map.
///
/// Instead: take the routed source's panel UV (the controller ray's hit, or the window-client
/// position [`UIManager::m_MouseX`] normalized against the window size), rescale it to the movie
/// rectangle (= our texture), and hand Scaleform the whole mouse state -- position plus buttons --
/// via `NotifyMouseState`, which diffs against the previous state on the next `Advance` to
/// synthesize hover/press/release. The wheel still needs an explicit event. The game's own
/// `MCI_cursor` sprite is parked offscreen (it would otherwise be drawn into the panel texture
/// alongside our own cursor), and the panel cursor's position and visibility are published for the
/// render side (see [`crate::hud::cursor`]).
///
/// Like the original, it runs both from `WndProc` (via `SetMousePos`) and once per frame from
/// `CUIManager::PreUpdate`, so the state stays fresh even while the physical mouse is still.
#[detour(address = UIManager::SendMouseEvents_ADDRESS)]
fn send_mouse_events(this: *mut UIManager, steering: *mut c_void) -> bool {
    let original = SEND_MOUSE_EVENTS.get().unwrap();
    // SAFETY: `this` is the live UI singleton (the engine's own callers pass it); the movie
    // pointer is checked before use.
    let manager = unsafe { this.as_mut() };
    let geometry = cursor::geometry();
    let (Some(manager), Some(((window_w, window_h), (movie_w, movie_h))), true) =
        (manager, geometry, mouse_route_active())
    else {
        // SAFETY: only the movie supplied by the current live manager is dereferenced.
        let blocked = debug_ui_capture().is_some_and(|snapshot| !snapshot.gameplay_effective);
        // SAFETY: only the current live manager's movie is used for cancellation.
        let movie = unsafe { this.as_mut().and_then(|manager| manager.m_Movie.as_mut()) };
        if blocked {
            cancel_pointer_for_ownership(movie);
            return true;
        }
        cancel_pointer(movie);
        NATIVE_MOUSE_CANCEL.lock().delivery();
        return original.call(this, steering);
    };
    let prepared = PREPARED_ROUTE.lock().take();
    let routed = prepared.map_or_else(
        || pointer::route_game_pointer(pointer::selected_controller()),
        PendingRoute::commit,
    );

    // SAFETY: the movie pointer comes from the live manager.
    let Some(movie) = (unsafe { manager.m_Movie.as_mut() }) else {
        cancel_pointer(None);
        return false;
    };
    let identity = movie_identity(movie);
    NATIVE_MOUSE_CANCEL.lock().delivery();
    if SYNTHETIC_MOUSE
        .lock()
        .movie
        .is_some_and(|previous| previous != identity)
    {
        // The previous movie is gone; nothing may be sent to it. Start the new one parked.
        cancel_pointer(None);
        park(movie);
        return true;
    }

    let desktop_uv = (
        (manager.m_MouseX as f32 / window_w as f32).clamp(0.0, 1.0),
        (manager.m_MouseY as f32 / window_h as f32).clamp(0.0, 1.0),
    );
    let desktop_capture = SYNTHETIC_MOUSE.lock().desktop_capture(identity);
    let Delivery::At {
        uv: (u, v),
        buttons,
    } = plan_delivery(
        routed,
        manager.m_IsUsingGamepad,
        desktop_capture,
        desktop_uv,
    )
    else {
        park(movie);
        SYNTHETIC_MOUSE.lock().delivered(identity, 0, false);
        cursor::take_wheel_lines();
        cursor::set_frame(None);
        return true;
    };

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

    // SAFETY: NotifyMouseState and HandleEvent are the movie's own embedding API, called from the
    // contexts the engine calls them from; the wheel event matches the layout the engine itself
    // builds for HandleEvent, and the overlay call no-ops unless the overlay is active.
    unsafe {
        movie.NotifyMouseState(x, y, buttons, 0);
        SYNTHETIC_MOUSE
            .lock()
            .delivered(identity, buttons, routed.is_controller());

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
        pointer_visible(routed.is_controller(), visible, manager.m_IsUsingGamepad)
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
    out: *mut Vector2,
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
        // The map's input runs inside `UIManager::PreUpdate`, before `SendMouseEvents`, so it reads
        // the controller cursor prepared at the start of this dispatch rather than the previous
        // delivery.
        let (u, v) = map::cursor_uv().map_or_else(
            || {
                (
                    (viewport_x / window.0 as f32).clamp(0.0, 1.0),
                    (viewport_y / window.1 as f32).clamp(0.0, 1.0),
                )
            },
            |uv| (uv.x, uv.y),
        );
        if let Some(out) = out.as_mut() {
            out.data = [u * stage_w, v * stage_h];
        }
    }
}

/// Whether the mod routes the game UI's mouse this frame: the HUD redirect and its cursor are on,
/// the redirect's mapping geometry is published, egui does not own input, and the payload is not
/// shutting down. The UI input boundary and the mouse delivery share this one gate.
fn mouse_route_active() -> bool {
    !crate::is_shutting_down()
        && Config::lock_query(|c| c.hud.redirect && c.hud.cursor.enabled)
        && debug_ui_capture().is_none_or(|snapshot| snapshot.gameplay_effective)
        && cursor::geometry().is_some()
}

/// The route prepared by the UI input boundary for this frame's delivery.
static PREPARED_ROUTE: Mutex<Option<PendingRoute>> = Mutex::new(None);

/// What was last delivered to which movie. Only the movie's address is kept, for comparison; a
/// saved movie pointer is never dereferenced.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
struct SyntheticMouse {
    movie: Option<usize>,
    buttons: u32,
    controller: bool,
}

static SYNTHETIC_MOUSE: Mutex<SyntheticMouse> = Mutex::new(SyntheticMouse {
    movie: None,
    buttons: 0,
    controller: false,
});

impl SyntheticMouse {
    fn delivered(&mut self, movie: usize, buttons: u32, controller: bool) {
        *self = Self {
            movie: Some(movie),
            buttons,
            controller,
        };
    }

    /// Whether a desktop press delivered to the live movie is still held, which keeps the desktop
    /// in charge through its release even in gamepad mode.
    fn desktop_capture(&self, live: usize) -> bool {
        self.movie == Some(live) && self.buttons != 0 && !self.controller
    }

    /// Forget the delivery, and report whether it left a button held on the live movie.
    fn cancel(&mut self, live: Option<usize>) -> bool {
        let held = self.buttons != 0 && self.movie.is_some() && self.movie == live;
        *self = Self::default();
        held
    }
}

/// Cancel every pointer source and forget the delivered state. A button still held on the live
/// `movie` is released off the movie, which Scaleform treats as a release outside, not a click. The
/// next controller press must follow a released sample, and a held desktop button must be released
/// before it counts again.
fn cancel_pointer(movie: Option<&mut MovieImpl>) {
    pointer::clear_controller();
    pointer::reset_route();
    cursor::set_frame(None);
    let live = movie.as_deref().map(movie_identity);
    if SYNTHETIC_MOUSE.lock().cancel(live)
        && let Some(movie) = movie
    {
        park(movie);
    }
}

/// Native input is not recorded in SyntheticMouse, so every ownership interval parks once.
fn cancel_pointer_for_ownership(movie: Option<&mut MovieImpl>) {
    pointer::clear_controller();
    pointer::reset_route();
    cursor::set_frame(None);
    let live = movie.as_deref().map(movie_identity);
    SYNTHETIC_MOUSE.lock().cancel(live);
    let cancellation = NATIVE_MOUSE_CANCEL.lock().cancel(live);
    if let (Some(Delivery::Park), Some(movie)) = (cancellation, movie) {
        park(movie);
    }
}

#[derive(Default)]
struct NativeMouseCancellation {
    parked_movie: Option<usize>,
}

impl NativeMouseCancellation {
    fn delivery(&mut self) {
        self.parked_movie = None;
    }

    fn cancel(&mut self, live: Option<usize>) -> Option<Delivery> {
        let should_park = live.is_some() && self.parked_movie != live;
        if should_park {
            self.parked_movie = live;
        }
        should_park.then_some(Delivery::Park)
    }
}

static NATIVE_MOUSE_CANCEL: Mutex<NativeMouseCancellation> =
    Mutex::new(NativeMouseCancellation { parked_movie: None });

fn movie_identity(movie: &MovieImpl) -> usize {
    ptr::from_ref(movie) as usize
}

/// Move the Scaleform mouse off the movie with no button held, where the engine parks it in
/// gamepad mode.
fn park(movie: &mut MovieImpl) {
    let parked = UIManager::GAMEPAD_PARKED_MOUSE_POSITION;
    // SAFETY: NotifyMouseState is the movie's own embedding API, called on the engine UI thread
    // with the live movie.
    unsafe { movie.NotifyMouseState(parked, parked, 0, 0) };
}

/// The mouse state to hand Scaleform for one delivery.
#[derive(Debug, Copy, Clone, PartialEq)]
enum Delivery {
    /// Park the mouse off the movie with no button held.
    Park,
    /// Place the mouse at a panel UV with the given buttons.
    At { uv: (f32, f32), buttons: u32 },
}

/// Turn a route into a delivery. In gamepad mode the desktop mouse parks like the engine's own
/// path, so gamepad-driven menu focus is not fought by a stale hover, unless a desktop press is
/// still held; the controller pointer is delivered in either mode.
fn plan_delivery(
    routed: RoutedPointer,
    gamepad: bool,
    desktop_capture: bool,
    desktop_uv: (f32, f32),
) -> Delivery {
    match routed {
        RoutedPointer::Parked => Delivery::Park,
        RoutedPointer::Controller { uv, buttons } => Delivery::At { uv, buttons },
        RoutedPointer::Desktop { .. } if gamepad && !desktop_capture => Delivery::Park,
        RoutedPointer::Desktop { buttons } => Delivery::At {
            uv: desktop_uv,
            buttons,
        },
    }
}

fn pointer_visible(controller: bool, desktop_visible: bool, gamepad: bool) -> bool {
    controller || (desktop_visible && !gamepad)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESKTOP: (f32, f32) = (0.1, 0.2);

    #[test]
    fn a_cancelled_controller_press_parks_instead_of_clicking() {
        assert_eq!(
            plan_delivery(RoutedPointer::Parked, false, false, DESKTOP),
            Delivery::Park
        );
        assert_eq!(
            plan_delivery(RoutedPointer::Parked, true, false, DESKTOP),
            Delivery::Park
        );
    }

    #[test]
    fn a_controller_release_outside_uses_the_current_hit() {
        let routed = RoutedPointer::Controller {
            uv: (1.0, 0.7),
            buttons: 0,
        };
        for gamepad in [false, true] {
            assert_eq!(
                plan_delivery(routed, gamepad, false, DESKTOP),
                Delivery::At {
                    uv: (1.0, 0.7),
                    buttons: 0
                }
            );
        }
    }

    #[test]
    fn gamepad_mode_parks_the_desktop_mouse_except_through_a_held_press() {
        let routed = RoutedPointer::Desktop { buttons: 1 };
        assert_eq!(plan_delivery(routed, true, false, DESKTOP), Delivery::Park);
        assert_eq!(
            plan_delivery(routed, true, true, DESKTOP),
            Delivery::At {
                uv: DESKTOP,
                buttons: 1
            }
        );
        assert_eq!(
            plan_delivery(routed, false, false, DESKTOP),
            Delivery::At {
                uv: DESKTOP,
                buttons: 1
            }
        );
    }

    #[test]
    fn only_a_held_desktop_press_on_the_live_movie_captures() {
        let mut state = SyntheticMouse::default();
        state.delivered(1, 1, false);
        assert!(state.desktop_capture(1));
        assert!(!state.desktop_capture(2));
        state.delivered(1, 1, true);
        assert!(!state.desktop_capture(1));
        state.delivered(1, 0, false);
        assert!(!state.desktop_capture(1));
    }

    #[test]
    fn cancellation_releases_only_the_live_movie_once() {
        for live in [Some(1), Some(2), None] {
            let mut state = SyntheticMouse::default();
            state.delivered(1, 1, true);
            assert_eq!(state.cancel(live), live == Some(1));
            assert!(!state.cancel(live));
            assert_eq!(state, SyntheticMouse::default());
        }
    }

    #[test]
    fn ownership_cancels_native_movie_input_once_without_a_synthetic_delivery() {
        let mut synthetic = SyntheticMouse::default();
        let mut native = NativeMouseCancellation::default();
        assert!(!synthetic.cancel(Some(1)));
        assert_eq!(native.cancel(None), None);
        assert_eq!(native.cancel(Some(1)), Some(Delivery::Park));
        assert_eq!(native.cancel(Some(1)), None);
        assert_eq!(native.cancel(Some(2)), Some(Delivery::Park));
        assert_eq!(native.cancel(Some(2)), None);
        native.delivery();
        assert_eq!(native.cancel(Some(2)), Some(Delivery::Park));
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
}
