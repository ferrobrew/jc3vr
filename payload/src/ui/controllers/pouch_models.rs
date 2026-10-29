//! The pouches' models: whether they are drawn, and which are.

use egui::Ui;

use crate::controllers::weapons::{
    config::PouchModelsConfig,
    pouch_models::{self, Prop},
};

/// The pouches' models' switch and readout.
pub(super) fn pouch_model_settings(ui: &mut Ui, m: &mut PouchModelsConfig) {
    ui.checkbox(&mut m.enabled, "Draw the pouches' models")
        .on_hover_text(
            "The game's grenade, C4 charge, and the held gun's magazine, drawn on their pouches \
             on Rico's body.",
        );
    let status = pouch_models::status();
    let shown: Vec<&str> = Prop::ALL
        .into_iter()
        .filter(|p| status.shown[p.index()])
        .map(Prop::label)
        .collect();
    ui.label(format!(
        "Drawn: {} · instances made {} · frames drawn {}",
        if shown.is_empty() {
            "none".to_owned()
        } else {
            shown.join(", ")
        },
        status.created,
        status.drawn
    ));
}
