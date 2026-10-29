//! The static foveated rendering tuning (issue #29).

use crate::config;

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut config::Config) {
    ui.add(
        egui::Slider::new(&mut cfg.foveation.inner_fraction, 0.0..=1.0)
            .text("Inner radius (fraction of half-diagonal, full-res inside)"),
    );
    ui.add(
        egui::Slider::new(&mut cfg.foveation.outer_fraction, 0.0..=1.5)
            .text("Outer radius (drop reaches max here)"),
    );
    ui.add(
        egui::Slider::new(&mut cfg.foveation.max_drop, 0.0..=1.0)
            .text("Max peripheral drop fraction"),
    );
    ui.checkbox(
        &mut cfg.foveation.debug_show_mask,
        "Debug: paint dropped pixels magenta (visualize the mask)",
    );
}
