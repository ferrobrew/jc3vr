//! Explicit saving and loading of controller settings, and the named test presets.

use egui::Ui;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        persistence::{self, Request, Status},
        presets::{Preset, apply_preset, preset_modified},
    },
};

static SELECTED: Mutex<Option<Preset>> = Mutex::new(None);

/// Draw the explicit save and load controls and the named test presets.
pub(super) fn settings(ui: &mut Ui, config: &mut Config) {
    let mut selected = SELECTED.lock();
    let status = persistence::status();
    ui.collapsing("Saved settings and test presets", |ui| {
        let location = persistence::file_path().map_or_else(
            || persistence::FILE_NAME.into(),
            |path| path.display().to_string(),
        );
        ui.label(format!(
            "Controller tuning is saved explicitly to {location}."
        ));
        ui.label("Body dimensions remain in body.local.json.");
        let busy = matches!(
            status,
            Status::Queued | Status::Running | Status::ShuttingDown
        );
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save controller settings").clicked() {
                    let file = persistence::from_config(config);
                    persistence::queue(Request::Save(Box::new(file)));
                }
                if ui.button("Load controller settings").clicked() {
                    persistence::queue(Request::Load);
                }
            });
        });
        ui.label("No controller settings are written automatically.");
        ui.label(
            "Loading replaces the controller and turn settings. Settings the file lacks take \
             their defaults, and settings this build does not know are ignored.",
        );
        ui.separator();
        ui.label(
            "Presets change feature switches only; tuning and sight calibration stay unchanged.",
        );
        ui.label(
            "Optional stick-click cycling, grenade fallback, finger source, and finger drive-all \
             settings are preserved.",
        );
        ui.horizontal_wrapped(|ui| {
            for preset in Preset::ALL {
                if ui
                    .selectable_label(*selected == Some(preset), preset.label())
                    .clicked()
                {
                    apply_preset(&mut config.controllers, preset);
                    *selected = Some(preset);
                }
            }
        });
        if let Some(preset) = *selected {
            let modified = if preset_modified(&config.controllers, preset) {
                " (modified)"
            } else {
                ""
            };
            ui.label(format!("Selected: {}{modified}", preset.label()));
        }
        status_line(ui, &status);
        if matches!(status, Status::Queued | Status::Running) {
            ui.ctx().request_repaint();
        }
    });
}

fn status_line(ui: &mut Ui, status: &Status) {
    match status {
        Status::Idle => {}
        Status::Queued => {
            ui.label("Controller settings: queued…");
        }
        Status::Running => {
            ui.label("Controller settings: working…");
        }
        Status::ShuttingDown => {
            ui.label("Controller settings: shutting down…");
        }
        Status::Finished(Ok(outcome)) => {
            ui.colored_label(
                egui::Color32::LIGHT_GREEN,
                format!(
                    "Controller settings: completed ({})",
                    outcome.path.display()
                ),
            );
            if !outcome.report.is_empty() {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    format!(
                        "{} missing settings took their defaults and {} unknown settings were \
                         ignored; the log lists them.",
                        outcome.report.defaulted.len(),
                        outcome.report.ignored.len()
                    ),
                );
            }
        }
        Status::Finished(Err(error)) => {
            ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
        }
    }
}
