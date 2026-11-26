//! The controls summary: what it shows now, why the panel is up, and its switches.

use egui::Ui;

use crate::{
    controllers::summary::{SummaryConfig, SummaryStatus},
    hud::summary_panel::PanelStatus,
    ui::controllers::yes_no,
};

pub(super) fn summary_readout(ui: &mut Ui, status: &SummaryStatus, panel: &PanelStatus) {
    ui.label(format!(
        "Showing: {} ({} rows, revision {}, language {:?})",
        if status.heading.is_empty() {
            "nothing yet"
        } else {
            &status.heading
        },
        status.rows,
        status.revision,
        status.locale
    ));
    ui.label(format!(
        "Current wrist projection: {} binding rows omitted by current eligibility",
        status.omitted,
    ));
    ui.label(format!(
        "Panel opacity {:.2} · glance {} · trackpad {} · mode change {}",
        panel.alpha,
        yes_no(panel.glance),
        yes_no(panel.pinned),
        yes_no(panel.mode_recent)
    ));
    match panel.layout {
        Some(layout) => ui.label(format!(
            "Panel texture {} × {} px, rows {:.0} px",
            layout.size.0, layout.size.1, layout.row_px
        )),
        None => ui.label("Panel texture: not laid out yet"),
    };
}

pub(super) fn summary_settings(ui: &mut Ui, s: &mut SummaryConfig) {
    ui.checkbox(&mut s.enabled, "Controls summary on the off-hand wrist")
        .on_hover_text("List what each controller input does in the current mode.");
    ui.add_enabled(
        s.enabled,
        egui::Checkbox::new(&mut s.show_on_glance, "Show on a glance at the wrist"),
    );
    ui.checkbox(&mut s.hide_game_hints, "Hide the game's button hints")
        .on_hover_text(
            "Hide the HUD's context button hints under the controller scheme: they show gamepad \
             glyphs for actions the controllers drive with other inputs.",
        );
}
