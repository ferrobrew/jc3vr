//! Registers the flat debug UI as the desktop overlay the engine's flip presents.

use jc3vr_render::hooks::graphics_engine::post_draw::register_desktop_overlay;

use crate::egui_impl::EguiState;

pub fn register() {
    register_desktop_overlay(render_flat);
}

/// Render the debug UI into the game's back buffer, unless the clean view hides it (the deferred
/// mirror-overlay path in `crate::hud::mirror_overlay` honours the same switch).
fn render_flat() {
    if let Some(egui_state) = EguiState::get().as_mut()
        && !jc3vr_core::director::clean_view::hides_desktop()
    {
        egui_state.render();
    }
}
