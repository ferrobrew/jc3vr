//! Grapple snatch tuning for distant objects, weapons, and doors.

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
            ui.add(egui::Slider::new(&mut s.range_m, 2.0..=60.0).text("Range (m)"));
            ui.add(
                egui::Slider::new(&mut s.cone_deg, 1.0..=20.0).text("Cone for guns in hands (°)"),
            );
            ui.add(
                egui::Slider::new(&mut s.yank.min_speed_mps, 0.5..=5.0).text("Yank speed (m/s)"),
            );
            ui.add(
                egui::Slider::new(&mut s.yank.min_distance_m, 0.05..=0.4).text("Yank length (m)"),
            );
            ui.add(egui::Slider::new(&mut s.yank.window_s, 0.05..=0.6).text("Yank window (s)"));
            ui.add(egui::Slider::new(&mut s.max_speed_mps, 2.0..=40.0).text("Fastest pull (m/s)"));
            ui.add(
                egui::Slider::new(&mut s.max_accel_mps2, 5.0..=300.0).text("Hardest pull (m/s²)"),
            );
            ui.add(
                egui::Slider::new(&mut s.stiffness_per_s, 0.5..=15.0).text("Pull stiffness (1/s)"),
            );
            ui.add(egui::Slider::new(&mut s.arc, 0.0..=0.8).text("Pull arc"));
            ui.add(
                egui::Slider::new(&mut s.capture_radius_m, 0.1..=1.0).text("Capture radius (m)"),
            );
            ui.add(
                egui::Slider::new(&mut s.stall_distance_m, 0.01..=0.5).text("Stall distance (m)"),
            );
            ui.add(egui::Slider::new(&mut s.stall_window_s, 0.1..=2.0).text("Stall window (s)"));
            ui.add(
                egui::Slider::new(&mut s.attach_timeout_s, 0.5..=6.0).text("Hook flight limit (s)"),
            );
            ui.checkbox(&mut s.holstered, "Guns holstered on characters")
                .on_hover_text(
                    "A character drops the gun on its body and staggers. Needs the steal.",
                );
            ui.checkbox(&mut s.props, "Loose objects")
                .on_hover_text("An object one hand can carry comes into the hand's hold.");
            ui.checkbox(&mut s.doors, "Car doors").on_hover_text(
                "The line pulls a closed door of a still vehicle open at its handle.",
            );
            ui.add_enabled_ui(s.doors, |ui| {
                ui.add(
                    egui::Slider::new(&mut s.door_reach_m, 0.1..=1.5).text("Door handle reach (m)"),
                );
                ui.add(
                    egui::Slider::new(&mut s.door_max_vehicle_speed_mps, 0.0..=10.0)
                        .text("Fastest vehicle (m/s)"),
                );
                ui.add(
                    egui::Slider::new(&mut s.door_swing_mps, 0.0..=10.0).text("Door swing (m/s)"),
                );
            });
        });
    });
}
