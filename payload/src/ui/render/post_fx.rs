//! The post-effect levers: the reprojection passes, and the individual stages to skip when
//! bisecting an artifact to one of them.

use crate::hooks::graphics_engine::{
    config::PostFxConfig, post::post_effects::vr_forces_blurs_off,
};

pub(super) fn section(ui: &mut egui::Ui, post_fx: &mut PostFxConfig) {
    ui.collapsing("Post-FX (reprojection passes, both eyes)", |ui| {
        // In VR the motion blur and the depth of field never run, whatever these say, so their
        // switches are locked rather than left to suggest they could be turned back on.
        let forced_off = vr_forces_blurs_off();
        if forced_off {
            ui.label("Motion blur and depth of field are forced off while VR renders.");
        }
        ui.add_enabled_ui(!forced_off, |ui| {
            ui.checkbox(
                &mut post_fx.skip_motion_blur,
                "Skip MotionBlur::Apply (whole pass)",
            );
            ui.checkbox(
                &mut post_fx.skip_motion_blur_recon,
                "Skip MotionBlur recon (if pass not skipped)",
            );
            ui.checkbox(
                &mut post_fx.skip_dof,
                "Skip DepthOfField::Apply, the tonemap included (washes out!)",
            );
        });
        ui.checkbox(
            &mut post_fx.dof_no_reproject,
            "DoF: plain composite, no reprojection (keeps picture)",
        );
    });

    ui.collapsing("Post-FX stages (skip to bisect)", |ui| {
        ui.checkbox(
            &mut post_fx.skip_histogram,
            "Exposure histogram (stalls auto-exposure)",
        );
        ui.checkbox(&mut post_fx.skip_glare, "Glare / bloom");
        ui.checkbox(&mut post_fx.skip_fade, "Fade");
        ui.checkbox(&mut post_fx.skip_sun_halo, "Sun halo");
        ui.checkbox(
            &mut post_fx.skip_anti_aliasing,
            "Anti-aliasing (when FSR is off)",
        );
        ui.checkbox(&mut post_fx.skip_player_damage, "Player-damage vignette");
    });
}
