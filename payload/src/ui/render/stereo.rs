//! The core stereo widgets: the double-Draw switch, FSR, and the shader reload that applies the
//! shader patches.

use crate::{
    config,
    hooks::graphics_engine::{shader, shadow_fit_gap_m},
};

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut config::Config) {
    ui.checkbox(&mut cfg.stereo.enabled, "Stereo (double-Draw)")
        .on_hover_text("Issue a second game.Draw per frame; CClock::Update is gated to once/frame");
    ui.checkbox(
        &mut cfg.fsr.enabled,
        "FSR anti-aliasing (replaces the engine SMAA)",
    );
    ui.checkbox(
        &mut cfg.stereo.shadow_anchor_to_fit,
        "Anchor shadows to the cascade fit's camera",
    )
    .on_hover_text(
        "Re-anchors the sun-shadow lookup at the camera position the cascade fit read, so a gap \
         between it and the render camera does not shift every shadow off its object. Off for an \
         A/B of shadows detached from distant objects while moving fast.",
    );
    ui.label(format!(
        "Render camera to the shadow fit's camera: {:.2} m",
        shadow_fit_gap_m()
    ));
    ui.checkbox(
        &mut cfg.stereo.shadow_fit_unpadded,
        "Fit shadow cascades to the eyes' exact field of view",
    )
    .on_hover_text(
        "Fits the sun-shadow cascades to the union of the two eyes' fields of view instead of the \
         padded cull frustum, for finer shadow texels. Off for an A/B.",
    );
    ui.horizontal(|ui| {
        if ui.button("Reload shaders").clicked() {
            shader::request_reload();
        }
        let patched = shader::patched_count();
        ui.label(if patched == 0 {
            "0 shadow PCF sites patched".to_string()
        } else {
            format!("{patched} shadow PCF sites patched")
        });
    })
    .response
    .on_hover_text(
        "Re-creates every shader so the shader patches take effect on shaders created before \
         injection (F11 toggles the shadow PCF patch and reloads).",
    );
}
