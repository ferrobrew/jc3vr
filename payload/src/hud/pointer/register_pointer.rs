//! The HUD's side of the controller pointers ([`crate::controllers::bindings::pointer::register_hud`]).
//!
//! The pointers run on the input tick, below the HUD. They read the HUD's settings, whether the
//! desktop mouse holds the game UI ([`crate::hud::pointer::route`]), and the egui panel's state,
//! and they move and toggle the egui panel ([`crate::hud::egui_panel`]), all through this table.

use crate::{
    config::Config,
    controllers::bindings::pointer::{EguiPointerSettings, PointerHud, register_hud},
    hud::{egui_panel, pointer::route},
};

pub(crate) fn register() {
    register_hud(&POINTER_HUD);
}

static POINTER_HUD: PointerHud = PointerHud {
    native_pointer_enabled,
    desktop_capture: route::desktop_capture,
    egui_pointer,
    panel_accepts_edit_input: egui_panel::accepts_edit_input,
    panel_is_placed: egui_panel::is_placed,
    begin_grab: egui_panel::begin_grab,
    hold_grab: egui_panel::hold_grab,
    end_grab: egui_panel::end_grab,
    toggle_panel: egui_panel::toggle,
};

fn native_pointer_enabled() -> bool {
    Config::lock_query(|c| c.hud.redirect && c.hud.quad)
}

fn egui_pointer() -> EguiPointerSettings {
    Config::lock_query(|c| EguiPointerSettings {
        enabled: c.hud.egui_panel.controller_pointer,
        aspect: c.hud.egui_panel.aspect,
    })
}
