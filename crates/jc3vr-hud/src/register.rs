//! Every registration the HUD makes with the layers below it, in one place: the game UI's
//! redirect, the panels the post-draw draws over each eye, the flat mirror overlay's offscreen
//! pass, the textures the desktop mirror composites, the flat debug UI the flip presents, the
//! debug UI's input capture, and the HUD's side of the controller pointers.
//!
//! The payload's composition calls [`register`] once, before the hooks are installed. Each
//! registration fills a slot of its own, so their order does not matter. [`register`] reports any
//! slot still empty to the install's registration check, which stops the startup naming it.

use jc3vr_core::registration::RegistrationCheck;
use jc3vr_gameplay::controllers::bindings::pointer::{hud_registered, register_hud};
use jc3vr_input::capture::{CaptureDriver, driver_registered, register_driver};
use jc3vr_view::{
    hooks::graphics_engine::post_draw::{
        EyeDraw, FramePass, UiRedirect, desktop_overlay_registered, eye_layers_registered,
        frame_pass_registered, register_desktop_overlay, register_eye_layers, register_frame_pass,
        register_ui_redirect, ui_redirect_registered,
    },
    vr::{MirrorLayers, mirror_layers_registered, register_mirror_layers},
};

use crate::{
    egui_impl,
    egui_impl::desktop_input::sample_desktop_input,
    hud,
    hud::{egui_panel, mirror_overlay, pointer, restart_notice, summary_panel},
};

/// Register the HUD with the layers below it, and report what is still missing to `check`.
pub fn register(check: &mut RegistrationCheck) {
    register_ui_redirect(UiRedirect {
        maintain: hud::tick,
        applied: hud::redirect_applied,
    });
    register_eye_layers(&EYE_LAYERS);
    register_frame_pass(FramePass::MirrorOverlay, mirror_overlay::render);
    register_mirror_layers(MirrorLayers {
        panel_srv: egui_panel::panel_srv,
        overlay_srv: mirror_overlay::overlay_srv,
    });
    register_desktop_overlay(egui_impl::render_flat);
    // The debug UI's state does not exist yet, which is harmless: the step returns `None` until it
    // is installed.
    register_driver(CaptureDriver {
        update: egui_impl::update_debug_ui_capture,
        sample_desktop: sample_desktop_input,
    });
    register_hud(&pointer::POINTER_HUD);

    check.require(
        "post-draw",
        "the game UI's redirect",
        ui_redirect_registered(),
    );
    check.require("post-draw", "the HUD's eye layers", eye_layers_registered());
    check.require(
        "post-draw",
        "the mirror overlay's frame pass",
        frame_pass_registered(FramePass::MirrorOverlay),
    );
    check.require(
        "desktop mirror",
        "the HUD's overlay textures",
        mirror_layers_registered(),
    );
    check.require(
        "post-draw",
        "the flat debug UI's desktop overlay",
        desktop_overlay_registered(),
    );
    check.require(
        "input capture",
        "the debug UI's driver",
        driver_registered(),
    );
    check.require(
        "controller pointer",
        "the HUD's pointer table",
        hud_registered(),
    );
}

/// The layers the HUD draws over each eye, over the render crate's world-space overlay and scope
/// lens, in draw order: the floating HUD panel, the egui debug panel, the controls summary, and the
/// restart notice on top.
static EYE_LAYERS: [EyeDraw; 4] = [
    hud::draw_quad,
    egui_panel::draw_quad,
    summary_panel::draw,
    restart_notice::draw,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registration_leaves_nothing_missing() {
        let mut check = RegistrationCheck::new();
        register(&mut check);
        assert_eq!(check.missing(), []);
    }
}
