//! Registers the HUD with the render layer: the game UI's redirect, the panels the post-draw draws
//! over each eye, the flat mirror overlay's offscreen pass, the textures the desktop mirror
//! composites, and the render camera the panel and the pointer share.

use crate::{
    hooks::{
        camera::register_render_camera_observer,
        graphics_engine::post_draw::{
            EyeLayer, FramePass, UiRedirect, register_eye_layer, register_frame_pass,
            register_ui_redirect,
        },
    },
    hud::{
        self, egui_panel, mirror_overlay, pointer::publish_camera, restart_notice, summary_panel,
    },
    vr::{MirrorLayers, register_mirror_layers},
};

pub(crate) fn register() {
    register_ui_redirect(UiRedirect {
        maintain: hud::tick,
        applied: hud::redirect_applied,
    });
    register_eye_layer(EyeLayer::HudPanel, hud::draw_quad);
    register_eye_layer(EyeLayer::DebugPanel, egui_panel::draw_quad);
    register_eye_layer(EyeLayer::ControlsSummary, summary_panel::draw);
    register_eye_layer(EyeLayer::RestartNotice, restart_notice::draw);
    register_frame_pass(FramePass::MirrorOverlay, mirror_overlay::render);
    register_mirror_layers(MirrorLayers {
        panel_srv: egui_panel::panel_srv,
        overlay_srv: mirror_overlay::overlay_srv,
    });
    register_render_camera_observer(publish_camera);
}
