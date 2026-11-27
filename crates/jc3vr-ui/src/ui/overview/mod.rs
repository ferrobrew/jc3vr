//! The Status feature: every status row (`crate::status`), with the rows that need attention
//! first.

#[cfg(test)]
mod tests;

use egui::{Response, RichText, Ui};
use jc3vr_core::session;

use crate::{
    status,
    status::{Health, Line},
    ui::{catalog::FeatureId, navigation::NavigationState, status_row::health_color},
};

pub(crate) fn render(ui: &mut Ui, selection: &mut NavigationState) {
    let snapshot = Snapshot::live();
    render_snapshot(ui, selection, &snapshot);
}

#[derive(Debug)]
struct Snapshot {
    sections: Vec<(&'static str, Vec<(&'static str, Line)>)>,
    session: Option<session::Location>,
}

impl Snapshot {
    fn live() -> Self {
        // Status rows may read configuration, so no configuration guard spans these reads.
        Self {
            sections: status::read_all().collect(),
            session: session::location(),
        }
    }

    fn rows(&self) -> Vec<Row<'_>> {
        self.sections
            .iter()
            .flat_map(|(section, lines)| {
                lines.iter().map(|(label, line)| Row {
                    section,
                    label,
                    line,
                })
            })
            .collect()
    }
}

#[derive(Debug)]
struct Row<'a> {
    section: &'static str,
    label: &'static str,
    line: &'a Line,
}

impl Row<'_> {
    fn priority(&self) -> u8 {
        match self.line.health {
            Health::Failed => 0,
            Health::Waiting => 1,
            Health::Ok => 2,
            Health::Off => 3,
        }
    }

    fn needs_attention(&self) -> bool {
        matches!(self.line.health, Health::Failed | Health::Waiting)
    }
}

/// Every row, failures first, then waiting rows, healthy rows, and intentionally off rows, each
/// group in section order.
fn ranked<'a>(rows: &'a [Row<'a>]) -> Vec<&'a Row<'a>> {
    let mut visible: Vec<_> = rows.iter().collect();
    visible.sort_by_key(|row| row.priority());
    visible
}

fn render_snapshot(ui: &mut Ui, selection: &mut NavigationState, snapshot: &Snapshot) {
    let rows = snapshot.rows();
    ui.heading("Needs attention");
    render_rows(
        ui,
        ranked(&rows)
            .into_iter()
            .filter(|row| row.needs_attention()),
        "No failures or waiting rows.",
    );
    egui::CollapsingHeader::new("Everything else")
        .default_open(true)
        .show(ui, |ui| {
            render_rows(
                ui,
                ranked(&rows)
                    .into_iter()
                    .filter(|row| !row.needs_attention()),
                "Every row needs attention.",
            );
        });
    ui.separator();
    session_location(ui, snapshot.session);
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        for (label, feature) in [
            ("Open performance", FeatureId::Previews),
            ("Open tracking capture", FeatureId::TrackingCapture),
            ("Open video recording", FeatureId::Video),
        ] {
            destination(ui, label, NavigationState { feature }, selection);
        }
        if ui
            .button("Log the summary")
            .on_hover_text("Write every row, with the notes the rows have no room for, to the log.")
            .clicked()
        {
            tracing::info!("status: {}", status::summary());
        }
    });
}

/// Where this session's files go, in both namespaces under wine, so they can be found on the host.
fn session_location(ui: &mut Ui, location: Option<session::Location>) {
    let Some(location) = location else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Session folder: unavailable; nothing is saved (see the log).",
        );
        return;
    };
    let source = match location.source {
        session::Source::Environment => session::ENV_VAR,
        session::Source::LocalData => "the local data directory",
    };
    ui.label(format!(
        "Session folder (from {source}): {}",
        location.windows.display()
    ));
    if let Some(unix) = location.unix {
        ui.label(format!("On the host: {unix}"));
    }
}

fn render_rows<'a>(ui: &mut Ui, rows: impl IntoIterator<Item = &'a Row<'a>>, empty: &str) {
    let mut shown = false;
    for row in rows {
        shown = true;
        ui.push_id((row.section, row.label), |ui| {
            ui.group(|ui| {
                ui.label(format!("{} / {}", row.section, row.label));
                let health = row.line.health;
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(health.word()).color(health_color(ui, health)));
                    ui.label(row.line.detail.as_str());
                });
                if let Some(note) = &row.line.note {
                    ui.label(note.as_str());
                }
            });
        });
    }
    if !shown {
        ui.label(empty);
    }
}

fn destination(
    ui: &mut Ui,
    label: &str,
    target: NavigationState,
    selection: &mut NavigationState,
) -> Response {
    let response = ui.button(label);
    if response.clicked() {
        *selection = target;
    }
    response
}
