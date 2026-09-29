//! Explicit controller settings persistence and named switch presets.

use egui::Ui;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::testing::{self as controller_testing, Preset, Request, Status},
};

static SELECTED: Mutex<Option<Preset>> = Mutex::new(None);

/// Draw the explicit save/load controls and the named controller test presets.
pub(super) fn settings(ui: &mut Ui, config: &mut Config) {
    let mut selected = SELECTED.lock();
    let status = controller_testing::status();
    ui.collapsing("Persistence and test presets", |ui| {
        ui.label(format!(
            "Controller tuning is stored explicitly in {}. Body dimensions remain in body.local.json.",
            controller_testing::FILE_NAME
        ));
        let busy = matches!(status, Status::Queued | Status::Running | Status::ShuttingDown);
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save controller settings").clicked() {
                    controller_testing::queue(Request::Save(Box::new(controller_testing::from_config(config))));
                }
                if ui.button("Load controller settings").clicked() {
                    controller_testing::queue(Request::Load);
                }
            });
        });
        ui.label("No controller settings are written automatically. Loading replaces the complete saved controller and turn settings.");
        ui.separator();
        ui.label("Presets change feature switches only; tuning and sight calibration stay unchanged.");
        ui.label("Optional stick-click cycling, grenade fallback, finger source, and finger drive-all settings are preserved.");
        ui.horizontal_wrapped(|ui| {
            for preset in Preset::ALL {
                if ui.selectable_label(*selected == Some(preset), preset.label()).clicked() {
                    controller_testing::apply_preset(&mut config.controllers, preset);
                    *selected = Some(preset);
                }
            }
        });
        if let Some(preset) = *selected {
            let modified = controller_testing::preset_modified(&config.controllers, preset);
            ui.label(format!("Selected: {}{}", preset.label(), if modified { " (modified)" } else { "" }));
        }
        match &status {
            Status::Idle => {}
            Status::Queued => { ui.label("Controller settings: queued…"); }
            Status::Running => { ui.label("Controller settings: working…"); }
            Status::ShuttingDown => { ui.label("Controller settings: shutting down…"); }
            Status::Finished(Ok(path)) => {
                ui.colored_label(egui::Color32::LIGHT_GREEN, format!("Controller settings: completed ({})", path.display()));
            }
            Status::Finished(Err(error)) => {
                ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
            }
        }
        if matches!(controller_testing::status(), Status::Queued | Status::Running) {
            ui.ctx().request_repaint();
        }
    });
}
