//! The player's body dimensions and where they came from.

use egui::Ui;

use crate::controllers::body::{self, BodySource};

pub(super) fn body_readout(ui: &mut Ui, body: &body::PlayerBody) {
    ui.label(match &body.source {
        BodySource::File { path } => format!("From {}", path.display()),
        BodySource::Defaults { path: Some(path) } => {
            format!("Generic adult defaults (no {})", path.display())
        }
        BodySource::Defaults { path: None } => "Generic adult defaults".to_owned(),
        BodySource::Invalid { error } => format!("Generic adult defaults ({error})"),
    });
    let d = body.dimensions;
    egui::Grid::new("body_dimensions").show(ui, |ui| {
        for (name, value) in [
            ("Height", d.height),
            ("Eye height", d.eye_height),
            ("Shoulder height", d.shoulder_height),
            ("Shoulder width", d.shoulder_width),
            ("Arm span", d.arm_span),
            ("Upper arm", d.upper_arm),
            ("Forearm", d.forearm),
            ("Hand", d.hand),
        ] {
            ui.label(name);
            ui.label(format!("{value:.2} m"));
            ui.end_row();
        }
    });
    if ui
        .button("Reload body file")
        .on_hover_text("Re-read body.local.json beside the payload DLL.")
        .clicked()
    {
        body::reload();
    }
}
