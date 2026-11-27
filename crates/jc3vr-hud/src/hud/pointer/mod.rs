//! Pointer sources for the game's floating UI and the interactive egui panel.
//!
//! The game's UI is Scaleform, not egui. The controller path therefore produces the same panel UV
//! and primary-button state as the desktop mouse, and [`crate::hooks::ui`] feeds both through the
//! engine's `MovieImpl::NotifyMouseState` route.
//!
//! - [`jc3vr_view::hud::panel`] holds the panels the HUD drew, the render camera the
//!   camera hook placed them from, and the ray intersection against them;
//! - the input tick turns the controllers into pointer samples
//!   ([`jc3vr_gameplay::controllers::bindings::pointer`]);
//! - [`route`] decides which source, if any, owns the game UI's mouse on each delivery.
//!
//! The egui debug panel has its own pointer, built the same way: the input tick queues the
//! controller's updates, and [`egui_route`] gives egui's pointer to the desktop mouse or the
//! controller, whichever was used last, and builds the egui events.
//!
//! The pointers' input side reads the HUD's redirect switch, the desktop mouse's capture, and the
//! debug panel's state through [`POINTER_HUD`], which the crate's registration installs
//! ([`crate::register`]).

mod egui_route;
mod route;

use jc3vr_gameplay::controllers::bindings::pointer::PointerHud;

pub use crate::hud::pointer::{
    egui_route::{
        apply_panel_input_options, egui_cursor_uv, egui_panel_events, reset_egui_pointer,
    },
    route::{PendingRoute, RoutedPointer, peek_route, reset_route, route_game_pointer},
};
use crate::hud::{config::HUD_CONFIG, cursor, egui_panel};

/// The panel-texture UV of the desktop mouse this frame, or `None` when no mouse position or window
/// size is known yet.
pub(crate) fn window_uv() -> Option<(f32, f32)> {
    let (mx, my) = cursor::mouse_pos()?;
    let (nw, nh) = cursor::normalization_size()?;
    let u = (mx as f32 / nw as f32).clamp(0.0, 1.0);
    let v = (my as f32 / nh as f32).clamp(0.0, 1.0);
    Some((u, v))
}

/// The HUD's side of the controller pointers.
pub(crate) static POINTER_HUD: PointerHud = PointerHud {
    native_pointer_enabled,
    desktop_capture: route::desktop_capture,
    panel_accepts_edit_input: egui_panel::accepts_edit_input,
    panel_is_placed: egui_panel::is_placed,
    begin_grab: egui_panel::begin_grab,
    hold_grab: egui_panel::hold_grab,
    end_grab: egui_panel::end_grab,
    toggle_panel: egui_panel::toggle,
};

fn native_pointer_enabled() -> bool {
    HUD_CONFIG.query(|c| c.redirect)
}
