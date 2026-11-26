//! Pointer sources for the game's floating UI and the interactive egui panel.
//!
//! The game's UI is Scaleform, not egui. The controller path therefore produces the same panel UV
//! and primary-button state as the desktop mouse, and [`crate::hooks::ui`] feeds both through the
//! engine's `MovieImpl::NotifyMouseState` route.
//!
//! - [`panel`](jc3vr_render::hud::pointer::panel), in the render crate, holds the rendered panels and the ray intersection against them;
//! - the input tick turns the controllers into pointer samples
//!   ([`jc3vr_gameplay::controllers::bindings::pointer`]), which this module re-exports;
//! - [`route`] decides which source, if any, owns the game UI's mouse on each delivery.
//!
//! The egui debug panel has its own pointer, built the same way: the input tick queues the
//! controller's updates, and [`egui_route`] gives egui's pointer to the desktop mouse or the
//! controller, whichever was used last, and builds the egui events.
//!
//! The pointers' input side reads the HUD's settings and panel state through the table
//! [`register_pointer`] installs.

mod egui_route;
pub mod register_pointer;
mod route;

pub use jc3vr_gameplay::controllers::bindings::pointer::{
    ControllerPointer, clear_controller, owns_menu_trigger, selected_controller,
};
pub use jc3vr_render::hud::pointer::panel::{
    RenderedCamera, RenderedPanel, publish_camera, publish_egui_panel, publish_panel,
    rendered_camera,
};

use crate::hud::cursor;
pub use crate::hud::pointer::{
    egui_route::{
        apply_panel_input_options, egui_cursor_uv, egui_panel_events, reset_egui_pointer,
    },
    route::{PendingRoute, RoutedPointer, peek_route, reset_route, route_game_pointer},
};

/// The panel-texture UV of the desktop mouse this frame, or `None` when no mouse position or window
/// size is known yet.
pub fn window_uv() -> Option<(f32, f32)> {
    let (mx, my) = cursor::mouse_pos()?;
    let (nw, nh) = cursor::normalization_size()?;
    let u = (mx as f32 / nw as f32).clamp(0.0, 1.0);
    let v = (my as f32 / nh as f32).clamp(0.0, 1.0);
    Some((u, v))
}
