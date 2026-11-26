//! The pouches' models: which are drawn.

use egui::Ui;
use jc3vr_weapons::controllers::weapons::{pouch_models, pouch_models::Prop};

/// The pouches' models' readout: the game's grenade, C4 charge (or detonator, once no charge is
/// left), and the held gun's magazine, drawn on their pouches on Rico's body, and the magazine a
/// hand takes from the pouch, drawn in the hand.
pub(super) fn pouch_model_readout(ui: &mut Ui) {
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
