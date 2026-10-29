//! Weapon selection: empty-weapon switching and the weapon wheel.

use egui::Ui;

use crate::controllers::config::WeaponsConfig;

pub(super) fn selection_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(
        &mut w.auto_switch_when_empty,
        "Switch weapons automatically when empty",
    )
    .on_hover_text(
        "Let the game switch to another weapon when the trigger of an empty weapon that cannot \
         reload is pulled. Off, the empty weapon stays in the hand and the pull only dry-fires.",
    );
    ui.checkbox(&mut w.wheel.enabled, "Weapon wheel")
        .on_hover_text(
            "Open the game's selection cross with a held input and choose an arm with the hand.",
        );
}
