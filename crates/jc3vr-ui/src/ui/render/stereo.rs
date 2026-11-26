//! The core stereo widgets: the double-Draw switch, FSR, and the shader reload that applies the
//! shader patches.

use jc3vr_core::config::engine::Parts;
use jc3vr_render::{
    fsr::{AntiAliasing, FSR_CONFIG},
    hooks::graphics_engine::{shader, shadow_fit_gap_m},
    stereo::config::STEREO_CONFIG,
};

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut impl Parts) {
    ui.checkbox(
        &mut cfg.part_mut(&STEREO_CONFIG).enabled,
        "Stereo (double-Draw)",
    )
    .on_hover_text("Issue a second game.Draw per frame; CClock::Update is gated to once/frame");
    ui.horizontal(|ui| {
        ui.label("Anti-aliasing");
        for (choice, label, hover) in [
            (
                AntiAliasing::Fsr,
                "FSR",
                "FSR 2 at 1:1 in place of the engine's SMAA",
            ),
            (
                AntiAliasing::Taa,
                "TAA",
                "The mod's own single-pass temporal resolve in place of the engine's SMAA: \
                 reprojected, variance-clipped history from the same jitter and motion vectors \
                 FSR uses",
            ),
            (
                AntiAliasing::Engine,
                "Engine",
                "The engine's own anti-aliasing (SMAA 1x in stereo), without the jitter",
            ),
        ] {
            ui.selectable_value(&mut cfg.part_mut(&FSR_CONFIG).antialiasing, choice, label)
                .on_hover_text(hover);
        }
    });
    if cfg.part_mut(&FSR_CONFIG).antialiasing == AntiAliasing::Taa {
        let taa = &mut cfg.part_mut(&FSR_CONFIG).taa;
        ui.add(egui::Slider::new(&mut taa.history_weight, 0.0..=0.98).text("TAA history at rest"))
            .on_hover_text("The history's share of each pixel with the pixel still");
        ui.add(
            egui::Slider::new(&mut taa.motion_history_weight, 0.0..=0.98)
                .text("TAA history in motion"),
        )
        .on_hover_text("The history's share once the pixel moves 16 pixels a frame or more");
        ui.add(egui::Slider::new(&mut taa.clip_gamma_rest, 0.5..=3.0).text("TAA clip box at rest"))
            .on_hover_text("The colour box's half-size in standard deviations, pixel still");
        ui.add(egui::Slider::new(&mut taa.clip_gamma, 0.5..=3.0).text("TAA clip box in motion"))
            .on_hover_text("The colour box's half-size in standard deviations, pixel moving");
        ui.add(
            egui::Slider::new(&mut taa.current_sharpness, 0.5..=16.0)
                .text("TAA current-frame sharpness"),
        )
        .on_hover_text("The falloff of the kernel that unjitters the current frame");
        ui.add(egui::Slider::new(&mut taa.depth_tolerance, 0.01..=0.5).text("TAA depth tolerance"))
            .on_hover_text("The relative depth slack before a history counts as disoccluded");
    }
    ui.checkbox(
        &mut cfg.part_mut(&FSR_CONFIG).jitter,
        "Sub-pixel jitter (FSR and TAA)",
    )
    .on_hover_text(
        "Moves each eye's projection by a sub-pixel offset of FSR's Halton sequence every frame \
             and tells the temporal resolve the offset, so its accumulation gathers new sample \
             positions rather than filtering one. Only while FSR or TAA runs. Off for an A/B.",
    );
    ui.checkbox(
        &mut cfg.part_mut(&FSR_CONFIG).stereo_mv_correction,
        "FSR stereo motion-vector correction",
    )
    .on_hover_text(
        "Adds the parallax between each eye and the centre camera to FSR's motion vectors, for a \
         velocity buffer written against the centre camera's previous frame. Only the velocity \
         pass's own draws write that buffer; every other pixel holds zero motion, which this turns \
         into the parallax. Off for an A/B.",
    );
    ui.checkbox(
        &mut cfg.part_mut(&FSR_CONFIG).hybrid_velocity,
        "FSR hybrid motion vectors (camera motion from depth)",
    )
    .on_hover_text(
        "Gives every pixel the velocity pass does not draw, and every pixel whose encoded motion hit \
         the encoding's clamp, the camera's motion from its depth through each eye's own previous \
         view-projection, keeping the engine's object motion where it wrote some. Off for an A/B.",
    );
    ui.checkbox(
        &mut cfg.part_mut(&STEREO_CONFIG).shadow_anchor_to_fit,
        "Anchor shadows to the cascade fit's camera",
    )
    .on_hover_text(
        "Re-anchors the sun-shadow lookup at the camera position the cascade fit read, so a gap \
         between it and the render camera does not shift every shadow off its object. Off for an \
         A/B of shadows detached from distant objects while moving fast.",
    );
    ui.checkbox(
        &mut cfg.part_mut(&STEREO_CONFIG).terrain_prep_from_centre,
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
        &mut cfg.part_mut(&STEREO_CONFIG).shadow_fit_unpadded,
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
