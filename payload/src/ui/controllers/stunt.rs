//! Stunt positions held by the grip: the position the game holds the player in, the entries and
//! let-gos, and the settings.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    physical_hands::stunt::{StuntConfig, StuntStatus},
};

pub(super) fn stunt_readout(ui: &mut Ui, status: &StuntStatus) {
    ui.label(format!(
        "{} · {}",
        status
            .position
            .map_or("not in a stunt position", |kind| kind.label()),
        if status.armed {
            "a grip held in this hang, so opening every grip lets go"
        } else {
            "no grip has held in this hang yet"
        }
    ));
    ui.label(format!(
        "{} hangs and {} roofs entered · {} let go · {} refused{}",
        status.hang_entries,
        status.roof_entries,
        status.releases,
        status.refusals,
        status.last_entry.map_or_else(String::new, |entry| format!(
            " · last entry onto {}",
            entry.label()
        ))
    ));
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let i = hand.index();
            ui.strong(hand.label());
            ui.label(if status.holding[i] {
                "holds the hang"
            } else if status.on_helicopter[i] {
                "grips a helicopter"
            } else {
                "holds no stunt handhold"
            });
        }
    });
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
}

pub(super) fn stunt_settings(ui: &mut Ui, c: &mut StuntConfig) {
    ui.checkbox(
        &mut c.enabled,
        "Hold stunt positions with the grip (needs the hand climbing)",
    );
    ui.add_enabled_ui(c.enabled, |ui| {
        ui.checkbox(&mut c.enter_hang, "Enter a helicopter's hang from a grip");
        ui.add(egui::Slider::new(&mut c.hang_hold_s, 0.0..=2.0).text("Hold before the hang (s)"));
        ui.add(
            egui::Slider::new(&mut c.hang_below_m, -0.5..=1.5)
                .text("Body below the grip to hang (m)"),
        );
        ui.checkbox(&mut c.enter_roof, "Enter a roof's stunt area from a mantle");
        ui.add(
            egui::Slider::new(&mut c.roof_offer_m, 0.0..=2.0).text("Feet from the stunt area (m)"),
        );
        ui.checkbox(&mut c.let_go, "Opening every grip lets go of the hang");
        ui.add(
            egui::Slider::new(&mut c.release_grace_s, 0.0..=1.0)
                .text("Grips open before letting go (s)"),
        );
        ui.add(egui::Slider::new(&mut c.retry_s, 0.0..=5.0).text("Wait after a refusal (s)"));
        ui.add(
            egui::Slider::new(&mut c.hold_full_speed_mps, 1.0..=100.0)
                .text("Speed of the strongest hold pulse (m/s)"),
        );
        ui.add(egui::Slider::new(&mut c.skid_diameter_m, 0.01..=0.1).text("Skid diameter (m)"));
    });
}
