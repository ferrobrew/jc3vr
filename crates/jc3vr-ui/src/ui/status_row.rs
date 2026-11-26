//! Status rows on a feature page: each row's state word and detail, read through the row's own
//! formatter, so a feature page's headline and the Status feature always say the same thing.

use egui::{Color32, RichText, Ui};

use crate::status::{Health, Row};

/// Each row as its label, its state word in the state's colour, and its detail, with the row's
/// longer note beneath it when it has one. A feature page shows these above the lines that add
/// detail the rows have no room for.
pub(crate) fn headlines(ui: &mut Ui, rows: &[&Row]) {
    for row in rows {
        let line = row.read();
        ui.horizontal_wrapped(|ui| {
            ui.strong(row.display_label());
            ui.label(
                RichText::new(line.health.display_word()).color(health_color(ui, line.health)),
            );
            if !line.display_detail.is_empty() {
                ui.label(line.display_detail.as_str());
            }
        });
        if let Some(note) = &line.display_note {
            ui.small(note.as_str());
        }
    }
}

/// The colour a state word is drawn in.
pub(crate) fn health_color(ui: &Ui, health: Health) -> Color32 {
    match health {
        Health::Failed => ui.visuals().error_fg_color,
        Health::Waiting => ui.visuals().warn_fg_color,
        Health::Ok => Color32::from_rgb(90, 190, 90),
        Health::Off => ui.visuals().weak_text_color(),
    }
}
