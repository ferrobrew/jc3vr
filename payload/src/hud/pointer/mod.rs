//! Pointer sources for the game's floating UI and the interactive egui panel.
//!
//! The game's UI is Scaleform, not egui. The controller path therefore produces the same panel UV
//! and primary-button state as the desktop mouse, and [`crate::hooks::ui`] feeds both through the
//! engine's `MovieImpl::NotifyMouseState` route.
//!
//! - [`panel`] holds the rendered panels and the ray intersection against them;
//! - [`controller`] turns the dominant controller's aim ray and trigger into a pointer sample;
//! - [`route`] decides which source, if any, owns the game UI's mouse on each delivery.
//!
//! The egui debug panel has its own pointer, built the same way:
//!
//! - [`egui_controller`] turns either controller's aim ray, trigger, upper face button, and
//!   thumbstick into pointer updates;
//! - [`egui_route`] gives egui's pointer to the desktop mouse or the controller, whichever was used
//!   last, and builds the egui events.

mod controller;
mod egui_controller;
mod egui_route;
mod panel;
mod route;

use crate::hud::cursor;
pub use crate::hud::pointer::{
    controller::{
        ControllerPointer, clear_controller, owns_menu_trigger, selected_controller,
        update_controller,
    },
    egui_route::{
        apply_panel_input_options, egui_cursor_uv, egui_panel_events, reset_egui_pointer,
    },
    panel::{RenderedPanel, publish_camera, publish_egui_panel, publish_panel, rendered_camera},
    route::{PendingRoute, RoutedPointer, peek_route, reset_route, route_game_pointer},
};
pub(crate) use crate::hud::pointer::{
    egui_controller::update_egui_controller,
    panel::{aim_at_panel, intersect_panel},
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
