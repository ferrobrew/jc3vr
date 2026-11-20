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
        &mut cfg.fsr.stereo_mv_correction,
        "FSR stereo motion-vector correction",
    )
    .on_hover_text(
        "Adds the parallax between each eye and the centre camera to FSR's motion vectors, for a \
         velocity buffer written against the centre camera's previous frame. Only the velocity \
         pass's own draws write that buffer; every other pixel holds zero motion, which this turns \
         into the parallax. Off for an A/B.",
    );
    ui.checkbox(
        &mut cfg.fsr.hybrid_velocity,
        "FSR hybrid motion vectors (camera motion from depth)",
    )
    .on_hover_text(
        "Gives every pixel the velocity pass does not draw, and every pixel whose encoded motion hit \
         the encoding's clamp, the camera's motion from its depth through each eye's own previous \
         view-projection, keeping the engine's object motion where it wrote some. Off for an A/B.",
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
    ui.checkbox(
        &mut cfg.stereo.terrain_prep_from_centre,
        "Terrain patch prep from the centre camera",
    )
    .on_hover_text(
        "Runs the terrain patches' per-frame detail and morph from the centre camera when the eyes \
         are drawn one at a time, as single-pass runs it once for both. Off, each eye morphs the \
         ground from its own position, so the eyes disagree about the surface by a fraction of a \
         millimetre.",
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
