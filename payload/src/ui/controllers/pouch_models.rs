//! The pouches' models: whether each is drawn, and where each sits on its pouch.

use egui::Ui;

use crate::controllers::weapons::{
    config::{ModelPlacement, PouchModelsConfig},
    pouch_models::{self, Prop},
};

/// The pouches' models' switch, readout, and placements.
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
    ui.label("Each model's place on its pouch, right side (+X right, +Y up the spine, +Z back)");
    placement_settings(ui, "Grenade", &mut m.grenade);
    placement_settings(ui, "C4 charge", &mut m.charge);
    placement_settings(ui, "Magazine", &mut m.magazine);
    if ui.button("Reset the models' places").clicked() {
        *m = PouchModelsConfig {
            enabled: m.enabled,
            ..PouchModelsConfig::new()
        };
    }
}

fn placement_settings(ui: &mut Ui, label: &str, p: &mut ModelPlacement) {
    for (axis, value) in [
        ("x", &mut p.offset_m.x),
        ("y", &mut p.offset_m.y),
        ("z", &mut p.offset_m.z),
    ] {
        ui.add(egui::Slider::new(value, -0.2..=0.2).text(format!("{label} offset {axis} (m)")));
    }
    for (axis, value) in [
        ("x", &mut p.rotation_deg.x),
        ("y", &mut p.rotation_deg.y),
        ("z", &mut p.rotation_deg.z),
    ] {
        ui.add(
            egui::Slider::new(value, -180.0..=180.0).text(format!("{label} turn about {axis} (°)")),
        );
    }
}
