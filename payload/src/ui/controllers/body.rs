//! The player's body dimensions and whether they are still the generic ones.

use egui::Ui;

use crate::controllers::body::BodyDimensions;

pub(super) fn body_readout(ui: &mut Ui, d: &BodyDimensions) {
    ui.label(if *d == BodyDimensions::GENERIC_ADULT {
        "Generic adult defaults; calibrate on the pause menu's VR: BODY page, or edit the [body] \
         section of the settings file."
    } else {
        "Saved in the [body] section of the settings file."
    });
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
        ui.label("Seated eye height");
        ui.label(
            d.seated_eye_height
                .map_or_else(|| "not calibrated".to_owned(), |h| format!("{h:.2} m")),
        );
        ui.end_row();
    });
}
