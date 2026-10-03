//! The controls summary: what it shows now, why the panel is up, and its settings.

use egui::Ui;

use crate::{
    controllers::summary::{SummaryConfig, SummaryStatus},
    hud::summary_panel::PanelStatus,
    ui::controllers::{vec3_drag, yes_no},
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
        "Panel opacity {:.2} · glance {} · trackpad {} · mode change {}",
        panel.alpha,
        yes_no(panel.glance),
        yes_no(panel.pinned),
        yes_no(panel.mode_recent)
    ));
}

pub(super) fn summary_settings(ui: &mut Ui, s: &mut SummaryConfig) {
    ui.checkbox(&mut s.hide_game_hints, "Hide the game's button hints")
        .on_hover_text(
            "Hide the HUD's context button hints under the controller scheme: they show gamepad \
             glyphs for actions the controllers drive with other inputs.",
        );
    ui.checkbox(&mut s.enabled, "Controls summary on the off-hand wrist")
        .on_hover_text("List what each controller input does in the current mode.");
    ui.add_enabled_ui(s.enabled, |ui| {
        ui.checkbox(&mut s.show_on_glance, "Show on a glance at the wrist");
        ui.checkbox(
            &mut s.show_on_trackpad_touch,
            "Show while the off-hand trackpad is touched",
        )
        .on_hover_text("Index controllers only.");
        ui.add(
            egui::Slider::new(&mut s.show_on_mode_change_s, 0.0..=10.0)
                .text("Show on a mode change (s)"),
        );
        let g = &mut s.glance;
        ui.add(egui::Slider::new(&mut g.view_cone_deg, 5.0..=60.0).text("Glance view cone (°)"));
        ui.add(egui::Slider::new(&mut g.facing_deg, 10.0..=90.0).text("Glance facing (°)"));
        ui.add(egui::Slider::new(&mut g.hysteresis_deg, 0.0..=30.0).text("Glance hysteresis (°)"));
        ui.add(egui::Slider::new(&mut g.dwell_s, 0.0..=1.0).text("Glance dwell (s)"));
        ui.add(egui::Slider::new(&mut g.linger_s, 0.0..=2.0).text("Glance linger (s)"));
        ui.add(egui::Slider::new(&mut s.fade_s, 0.0..=1.0).text("Fade (s)"));
        vec3_drag(ui, "Wrist offset (m)", &mut s.placement.offset_m, 0.005);
        ui.add(egui::Slider::new(&mut s.placement.width_m, 0.05..=0.4).text("Panel width (m)"));
    });
}
