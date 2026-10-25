//! Weapon selection, empty-weapon switching, and hand-driven wheel tuning.

use egui::Ui;

use crate::controllers::{
    config::WeaponsConfig,
    weapons::wheel::config::{SteerMode, WheelConfig},
};

pub(super) fn selection_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(
        &mut w.auto_switch_when_empty,
        "Switch weapons automatically when empty",
    )
    .on_hover_text(
        "Let the game switch to another weapon when the trigger of an empty weapon that cannot \
         reload is pulled. Off, the empty weapon stays in the hand and the pull only dry-fires.",
    );
    ui.checkbox(&mut w.fire_draws_weapon, "Fire draws a weapon")
        .on_hover_text(
            "Let the trigger draw the selected weapon with nothing in the hands, as the game does. \
             Off, weapons come off the body's sockets.",
        );
    ui.checkbox(&mut w.wheel.enabled, "Weapon wheel")
        .on_hover_text(
            "Open the game's selection cross with a held input and choose an arm with the hand.",
        );
    ui.add_enabled_ui(w.wheel.enabled, |ui| {
        ui.indent("wheel", |ui| wheel_settings(ui, &mut w.wheel));
    });
}

fn wheel_settings(ui: &mut Ui, w: &mut WheelConfig) {
    ui.checkbox(
        &mut w.trackpad,
        "Open with the weapon-select trackpad press",
    );
    ui.checkbox(&mut w.stick, "Open with the weapon wheel stick push");
    ui.add_enabled_ui(w.stick, |ui| {
        ui.add(egui::Slider::new(&mut w.stick_open, 0.0..=1.0).text("Stick opens at"));
        ui.add(egui::Slider::new(&mut w.stick_release, 0.0..=1.0).text("Stick closes below"));
    });
    ui.horizontal(|ui| {
        ui.label("Steer by");
        ui.selectable_value(&mut w.steer, SteerMode::Point, "Pointing the hand");
        ui.selectable_value(&mut w.steer, SteerMode::Offset, "Moving the hand");
    });
    match w.steer {
        SteerMode::Point => {
            ui.add(
                egui::Slider::new(&mut w.point_leave_deg, 1.0..=45.0)
                    .text("Leave the centre at (°)"),
            );
            ui.add(
                egui::Slider::new(&mut w.point_return_deg, 0.0..=45.0)
                    .text("Return to the centre at (°)"),
            );
        }
        SteerMode::Offset => {
            ui.add(
                egui::Slider::new(&mut w.offset_leave_m, 0.005..=0.2)
                    .text("Leave the centre at (m)"),
            );
            ui.add(
                egui::Slider::new(&mut w.offset_return_m, 0.0..=0.2)
                    .text("Return to the centre at (m)"),
            );
        }
    }
    ui.add(egui::Slider::new(&mut w.hysteresis_deg, 0.0..=44.0).text("Arm hysteresis (°)"));
    ui.checkbox(&mut w.place_at_hand, "Place the cross where the hand was");
}
