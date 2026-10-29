//! The core stereo widgets: the double-Draw switch, FSR, and the shader reload that applies the
//! shader patches.

use crate::{config, hooks::graphics_engine::shader};

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut config::Config) {
    ui.checkbox(&mut cfg.stereo.enabled, "Stereo (double-Draw)")
        .on_hover_text("Issue a second game.Draw per frame; CClock::Update is gated to once/frame");
    ui.checkbox(
        &mut cfg.fsr.enabled,
        "FSR anti-aliasing (replaces the engine SMAA)",
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
