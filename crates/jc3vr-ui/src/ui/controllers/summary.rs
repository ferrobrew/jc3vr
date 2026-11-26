//! The controls summary: what it shows now, and why the panel is up.

use egui::Ui;
use jc3vr_gameplay::controllers::summary::SummaryStatus;
use jc3vr_hud::hud::summary_panel::PanelStatus;

use crate::ui::controllers::yes_no;

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
