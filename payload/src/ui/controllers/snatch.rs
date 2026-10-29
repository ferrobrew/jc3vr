//! The grapple snatch's switches and the yank that fires it.

use egui::Ui;

use crate::controllers::snatch::SnatchConfig;

pub(super) fn snatch_settings(ui: &mut Ui, s: &mut SnatchConfig) {
    ui.checkbox(&mut s.enabled, "Grapple snatch").on_hover_text(
        "Grip with the empty grapple hand at a weapon out of reach under the grapple's aim, \
             then yank the hand back: the hook pulls the weapon into the hand. The grapple's aim \
             also sees weapons on the ground: a tap tosses one to your feet, a hold tethers it.",
    );
    ui.add_enabled_ui(s.enabled, |ui| {
        ui.indent("snatch", |ui| {
            ui.checkbox(&mut s.sling, "Grapple-held hammer throws").on_hover_text(
                "Grip a remote weapon or loose object, then hold B and yank. Swing it on the line; open the grip to throw, or release B to bring it to the hand.",
            );
            ui.add(
                egui::Slider::new(&mut s.yank.min_speed_mps, 0.5..=5.0).text("Yank speed (m/s)"),
            );
            ui.add(
                egui::Slider::new(&mut s.yank.min_distance_m, 0.05..=0.4).text("Yank length (m)"),
            );
        });
    });
}
