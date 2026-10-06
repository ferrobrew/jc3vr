//! Hand bodies: each hand body's phase and drive, and their settings.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    physical_hands::physics::{BodyPhase, HandBodiesConfig, HandBodiesStatus},
};

pub(super) fn hand_bodies_readout(ui: &mut Ui, status: &HandBodiesStatus) {
    if status.failed {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            "Stopped: the pre-simulation step panicked (see the log)",
        );
    }
    ui.label(format!(
        "Callback {} · {} steps · player group {}",
        if status.registered {
            "registered"
        } else {
            "not registered"
        },
        status.steps,
        status
            .system_group
            .map_or_else(|| "none".to_owned(), |g| g.to_string()),
    ));
    ui.label(format!(
        "{} created · {} destroyed",
        status.created, status.destroyed
    ));
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let s = &status.hands[hand.index()];
            ui.strong(hand.label());
            ui.label(match (s.body_id, s.filter_info) {
                (Some(id), Some(filter)) => {
                    format!("{} · body {id} · filter {filter:#010x}", s.phase.label())
                }
                (Some(id), None) => format!("{} · body {id}", s.phase.label()),
                _ => s.phase.label().to_owned(),
            });
            if s.phase == BodyPhase::Acting {
                ui.label(format!(
                    "lag {:.0} mm · {:.2} m/s · pull {:.0}% · twist {:.0}%",
                    s.lag_m * 1000.0,
                    s.speed_mps,
                    s.force_share * 100.0,
                    s.torque_share * 100.0
                ));
            }
            ui.label(format!("{} resets", s.resets));
        }
    });
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
}

pub(super) fn hand_bodies_settings(ui: &mut Ui, b: &mut HandBodiesConfig) {
    ui.checkbox(&mut b.enabled, "Hand bodies (in place of the palm probes)");
    ui.add_enabled_ui(b.enabled, |ui| {
        ui.add(egui::Slider::new(&mut b.strength, 0.0..=3.0).text("Strength (×)"));
        ui.add(egui::Slider::new(&mut b.mass_kg, 1.0..=50.0).text("Hand mass (kg)"));
        ui.add(egui::Slider::new(&mut b.max_force_n, 0.0..=20_000.0).text("Strongest pull (N)"));
        ui.add(egui::Slider::new(&mut b.max_torque_nm, 0.0..=2000.0).text("Strongest twist (N·m)"));
        ui.add(egui::Slider::new(&mut b.linear_gain, 0.0..=1.0).text("Position gain"));
        ui.add(egui::Slider::new(&mut b.angular_gain, 0.0..=1.0).text("Rotation gain"));
        ui.add(egui::Slider::new(&mut b.max_speed_mps, 0.0..=50.0).text("Fastest hand (m/s)"));
        ui.add(
            egui::Slider::new(&mut b.max_angular_speed_rps, 0.0..=100.0)
                .text("Fastest turn (rad/s)"),
        );
        ui.add(egui::Slider::new(&mut b.radius_m, 0.01..=0.1).text("Capsule radius (m)"));
        ui.add(egui::Slider::new(&mut b.half_length_m, 0.0..=0.1).text("Capsule half length (m)"));
        ui.add(egui::Slider::new(&mut b.break_distance_m, 0.1..=2.0).text("Break distance (m)"));
        ui.checkbox(&mut b.render_at_body, "Draw the hands at their bodies");
    });
}
