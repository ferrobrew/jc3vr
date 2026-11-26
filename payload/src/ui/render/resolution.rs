//! The static foveated rendering tuning (issue #29).

use crate::vr::FoveationConfig;

pub(super) fn section(ui: &mut egui::Ui, foveation: &mut FoveationConfig) {
    ui.add(
        egui::Slider::new(&mut foveation.inner_fraction, 0.0..=1.0)
            .text("Inner radius (fraction of half-diagonal, full-res inside)"),
    );
    ui.add(
        egui::Slider::new(&mut foveation.outer_fraction, 0.0..=1.5)
            .text("Outer radius (drop reaches max here)"),
    );
    ui.add(
        egui::Slider::new(&mut foveation.max_drop, 0.0..=1.0).text("Max peripheral drop fraction"),
    );
    ui.checkbox(
        &mut foveation.debug_show_mask,
        "Debug: paint dropped pixels magenta (visualize the mask)",
    );
}
