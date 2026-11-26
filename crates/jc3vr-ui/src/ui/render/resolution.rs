//! The static foveated rendering diagnostic (issue #29).

use jc3vr_view::vr::FoveationConfig;

pub(super) fn section(ui: &mut egui::Ui, foveation: &mut FoveationConfig) {
    ui.checkbox(
        &mut foveation.debug_show_mask,
        "Debug: paint dropped pixels magenta (visualize the mask)",
    );
}
