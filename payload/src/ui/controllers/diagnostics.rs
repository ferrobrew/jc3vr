//! The controller diagnostics history and on-demand JSON capture controls.

use egui::Ui;

use crate::controllers::diagnostics;

/// Render the opt-in controller diagnostics controls.
///
/// Register this section from `crate::ui::controllers` with
/// `diagnostics::diagnostics(ui)`. The collector itself is independent of whether this panel is
/// open, and the game hook records samples only after the collector is enabled.
pub(super) fn diagnostics(ui: &mut Ui) {
    let status = diagnostics::status();
    let mut enabled = status.enabled;
    let label = format!(
        "Enable bounded controller diagnostics (up to {} samples at {} Hz)",
        diagnostics::MAX_SAMPLES,
        diagnostics::SAMPLE_RATE_HZ
    );
    if ui.checkbox(&mut enabled, label).changed() {
        diagnostics::set_enabled(enabled);
    }
    ui.label(format!(
        "Recent samples: {} / {}",
        status.sample_count,
        diagnostics::MAX_SAMPLES
    ));
    if status.capture_pending {
        ui.label("Controller diagnostics capture: writing…");
    }
    if ui
        .add_enabled(
            status.sample_count > 0 && !status.capture_pending,
            egui::Button::new("Capture controller diagnostics"),
        )
        .on_hover_text(
            "Write the recent structured controller history to a new timestamped file in this \
             session's controller_diagnostics folder.",
        )
        .clicked()
        && let Err(error) = diagnostics::capture()
    {
        tracing::warn!(target: "controllers", "{error}");
    }
    if let Some(path) = status.last_path {
        ui.label(format!("Last capture: {path}"));
    }
    if let Some(error) = status.last_error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
}
