//! The Status tab shows authored display labels and status text from [`crate::status`].
//! The summary button logs the separate legacy text used by logs and scenario probes.

use egui::{Color32, RichText, Ui};

use crate::status::{self, Health, Line};

pub fn egui_debug_status(ui: &mut Ui) {
    if ui
        .button("Log the summary")
        .on_hover_text("Write every row, with the notes the rows have no room for, to the log.")
        .clicked()
    {
        tracing::info!("status: {}", status::summary());
    }
    ui.separator();
    // The rows read the configuration, so no configuration lock is held while they are read.
    let sections: Vec<_> = status::read_all().collect();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (id, title, rows) in sections {
            egui::CollapsingHeader::new(title)
                .id_salt(id)
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new(id).striped(true).show(ui, |ui| {
                        for (label, line) in rows {
                            row(ui, label, &line);
                        }
                    });
                });
        }
    });
}

fn row(ui: &mut Ui, label: &str, line: &Line) {
    ui.label(label);
    ui.label(
        RichText::new(line.health.display_word())
            .strong()
            .color(health_color(ui, line.health)),
    );
    let detail = ui.label(line.display_detail.as_str());
    if let Some(note) = &line.display_note {
        detail.on_hover_text(note.as_str());
    }
    ui.end_row();
}

fn health_color(ui: &Ui, health: Health) -> Color32 {
    match health {
        Health::Ok => Color32::from_rgb(90, 190, 90),
        Health::Off => ui.visuals().weak_text_color(),
        Health::Waiting => ui.visuals().warn_fg_color,
        Health::Failed => ui.visuals().error_fg_color,
    }
}
