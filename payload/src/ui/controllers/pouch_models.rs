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
             on Rico's body, and the magazine a hand takes from the pouch, drawn in the hand. \
             A shell or a revolver's rounds, which the game has no model of, are drawn in the \
             hand with the casing the gun ejects.",
        );
    let status = pouch_models::status();
    let mut shown: Vec<String> = Prop::ALL
        .into_iter()
        .filter(|p| status.shown[p.index()])
        .map(|p| p.label().to_owned())
        .collect();
    match (status.hand, status.hand_rounds) {
        (false, _) => {}
        (true, 0) => shown.push("magazine in the hand".to_owned()),
        (true, 1) => shown.push("a round in the hand".to_owned()),
        (true, n) => shown.push(format!("{n} rounds in the hand")),
    }
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
