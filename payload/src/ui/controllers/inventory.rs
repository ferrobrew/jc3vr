//! The VR inventory: every weapon it tracks and where each is, the pins, and its settings.

use egui::Ui;

use crate::controllers::weapons::{
    holster::slot_label,
    inventory::{InventoryStatus, config::InventoryConfig},
};

pub(super) fn inventory_readout(ui: &mut Ui, s: &InventoryStatus) {
    let (pinned, world) = s.world();
    ui.label(format!(
        "Active: {} · {} weapons · {pinned} of {world} in the world kept · pins {} · left to the \
         game {} · resets {}",
        super::yes_no(s.active),
        s.entries.len(),
        s.pins,
        s.forgotten,
        s.resets
    ));
    for entry in &s.entries {
        ui.label(format!(
            "#{} {} ({:#010x}) · {} · magazine {}{}",
            entry.id.0,
            slot_label(entry.kind.slot),
            entry.kind.tag_hash,
            entry.location.label(),
            entry.magazine,
            entry
                .distance_m
                .map_or(String::new(), |d| format!(" · {d:.1} m away"))
        ));
    }
}

pub(super) fn inventory_settings(ui: &mut Ui, c: &mut InventoryConfig) {
    ui.checkbox(&mut c.enabled, "Track the player's weapons");
    ui.add_enabled_ui(c.enabled, |ui| {
        ui.checkbox(
            &mut c.pin_world,
            "Keep dropped weapons nearby from despawning",
        );
        ui.add_enabled_ui(c.pin_world, |ui| {
            ui.add(egui::Slider::new(&mut c.pin_radius_m, 0.0..=30.0).text("Kept within (m)"));
            ui.add(egui::Slider::new(&mut c.unpin_radius_m, 0.0..=40.0).text("Let go of past (m)"));
            ui.add(egui::Slider::new(&mut c.max_pinned, 0..=16).text("Most kept at once"));
        });
        ui.add(egui::Slider::new(&mut c.forget_m, 10.0..=200.0).text("Left to the game past (m)"));
    });
}
