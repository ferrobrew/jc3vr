//! Pointer sources for the game's floating UI and the interactive egui panel.
//!
//! The game's UI is Scaleform, not egui. The controller path therefore produces the same panel UV
//! and primary-button state as the desktop mouse, and [`crate::hooks::ui`] feeds both through the
//! engine's `MovieImpl::NotifyMouseState` route.
//!
//! - [`panel`] holds the rendered panel and the ray intersection against it;
//! - [`controller`] turns the dominant controller's aim ray and trigger into a pointer sample;
//! - [`route`] decides which source, if any, owns the game UI's mouse on each delivery.
//!
//! The desktop mouse also drives the egui panel through [`window_mouse_events`].

mod controller;
mod panel;
mod route;

use std::sync::atomic::{AtomicU32, Ordering};

use crate::hud::cursor;
pub use crate::hud::pointer::{
    controller::{
        ControllerPointer, clear_controller, owns_menu_trigger, selected_controller,
        update_controller,
    },
    panel::{RenderedPanel, publish_camera, publish_panel, rendered_camera},
    route::{PendingRoute, RoutedPointer, peek_route, reset_route, route_game_pointer},
};

/// The panel-texture UV of the desktop mouse this frame, or `None` when no mouse position or window
/// size is known yet. Shared by the egui event pump and the panel's own cursor dot so they agree.
pub fn window_uv() -> Option<(f32, f32)> {
    let (mx, my) = cursor::mouse_pos()?;
    let (nw, nh) = cursor::normalization_size()?;
    let u = (mx as f32 / nw as f32).clamp(0.0, 1.0);
    let v = (my as f32 / nh as f32).clamp(0.0, 1.0);
    Some((u, v))
}

/// Build the egui pointer events for the VR panel from the desktop mouse: a move to the current UV,
/// press/release for any button that changed since the last call, and any accumulated wheel motion.
/// Positions are `uv * panel_size` so they land in the panel texture's coordinate space (the same
/// space egui lays out in while [`crate::egui_impl::EguiState::set_panel_mode`] is active). Call once
/// per frame from the game thread before [`crate::egui_impl::EguiState::run`].
pub fn window_mouse_events(panel_size: (u32, u32)) -> Vec<egui::Event> {
    let mut events = Vec::new();
    let Some((u, v)) = window_uv() else {
        // Still drain the wheel so it does not burst on the next frame a position is known.
        let _ = cursor::take_wheel_notches();
        PREV_BUTTONS.store(cursor::buttons(), Ordering::Relaxed);
        return events;
    };
    let pos = egui::Pos2::new(u * panel_size.0 as f32, v * panel_size.1 as f32);
    events.push(egui::Event::PointerMoved(pos));

    let buttons = cursor::buttons();
    let prev = PREV_BUTTONS.swap(buttons, Ordering::Relaxed);
    let changed = buttons ^ prev;
    for (bit, button) in [
        (
            1u32 << (cursor::Button::Left as u32),
            egui::PointerButton::Primary,
        ),
        (
            1u32 << (cursor::Button::Right as u32),
            egui::PointerButton::Secondary,
        ),
    ] {
        if changed & bit != 0 {
            events.push(egui::Event::PointerButton {
                pos,
                button,
                pressed: buttons & bit != 0,
                modifiers: egui::Modifiers::default(),
            });
        }
    }

    let notches = cursor::take_wheel_notches();
    if notches != 0.0 {
        events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::Vec2::new(0.0, notches),
            // Discrete wheel notches carry no gesture phase.
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        });
    }

    events
}

/// The previous frame's button bitmask (same layout as [`cursor::buttons`]), for edge detection.
static PREV_BUTTONS: AtomicU32 = AtomicU32::new(0);
