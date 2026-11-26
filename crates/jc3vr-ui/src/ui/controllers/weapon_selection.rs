//! Weapon selection: the weapon wheel.

use egui::Ui;
use jc3vr_weapons::controllers::weapons::config::WeaponsConfig;

pub(super) fn selection_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(&mut w.wheel.enabled, "Weapon wheel")
        .on_hover_text(
            "Open the game's selection cross with a held input and choose an arm with the hand.",
        );
}
