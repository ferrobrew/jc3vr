//! The named test presets for the controller feature switches.

use egui::Ui;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::presets::{Preset, apply_preset, preset_modified},
};

static SELECTED: Mutex<Option<Preset>> = Mutex::new(None);

/// Draw the named test presets.
pub(super) fn presets(ui: &mut Ui, config: &mut Config) {
    let mut selected = SELECTED.lock();
    ui.collapsing("Test presets", |ui| {
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
    });
}
