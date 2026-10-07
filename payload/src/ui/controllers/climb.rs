//! Hand climbing: what each hand reaches and grips, how the body is moved, and the settings.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    physical_hands::climb::{ClimbConfig, ClimbStatus, Hold},
};

pub(super) fn climb_readout(ui: &mut Ui, status: &ClimbStatus) {
    ui.label(format!(
        "{} · velocity ({:+.2}, {:+.2}, {:+.2}) m/s · gap {:.0} mm · game says {}",
        match (status.climbing, status.mantling) {
            (_, true) => "mantling",
            (true, false) => "climbing",
            (false, false) => "not climbing",
        },
        status.velocity.x,
        status.velocity.y,
        status.velocity.z,
        status.gap_m * 1000.0,
        match status.in_air {
            Some(true) => "in the air",
            Some(false) => "on the ground",
            None => "nothing",
        }
    ));
    ui.label(format!(
        "{} grips · {} releases · {} mantles · {} dropped{}",
        status.grips,
        status.releases,
        status.mantles,
        status.drops,
        status
            .last_drop
            .map_or_else(String::new, |reason| format!(" (last: {reason})"))
    ));
    if let Some(speed) = status.last_fling_mps {
        ui.label(format!("Last fling: {speed:.2} m/s"));
    }
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let s = &status.hands[hand.index()];
            ui.strong(hand.label());
            ui.label(match s.grip {
                Some(hold) => format!("grips {}", hold_label(&hold)),
                None => "grips nothing".to_owned(),
            });
            ui.label(match s.reach {
                Some(hold) => format!("reaches {}", hold_label(&hold)),
                None => "reaches nothing".to_owned(),
            });
        }
    });
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
}

pub(super) fn climb_settings(ui: &mut Ui, c: &mut ClimbConfig) {
    ui.checkbox(
        &mut c.enabled,
        "Climb by gripping the world (needs the hand bodies)",
    );
    ui.add_enabled_ui(c.enabled, |ui| {
        ui.add(egui::Slider::new(&mut c.reach_m, 0.0..=0.3).text("Reach past the palm (m)"));
        ui.add(egui::Slider::new(&mut c.gain, 0.0..=1.0).text("Follow gain"));
        ui.add(egui::Slider::new(&mut c.max_speed_mps, 0.0..=20.0).text("Fastest climb (m/s)"));
        ui.add(egui::Slider::new(&mut c.break_distance_m, 0.1..=3.0).text("Let go beyond (m)"));
        ui.separator();
        ui.checkbox(&mut c.fling, "Keep the speed on release");
        ui.add(egui::Slider::new(&mut c.fling_scale, 0.0..=3.0).text("Fling (×)"));
        ui.add(
            egui::Slider::new(&mut c.max_fling_speed_mps, 0.0..=30.0).text("Fastest fling (m/s)"),
        );
        ui.add(egui::Slider::new(&mut c.fling_smoothing_s, 0.0..=0.5).text("Fling smoothing (s)"));
        ui.separator();
        ui.checkbox(&mut c.mantle, "Mantle over ledges");
        ui.add(
            egui::Slider::new(&mut c.mantle_start_m, 0.0..=2.0)
                .text("Start with the feet below the top (m)"),
        );
        ui.add(egui::Slider::new(&mut c.mantle_clearance_m, 0.0..=0.5).text("Clearance (m)"));
        ui.add(egui::Slider::new(&mut c.mantle_forward_m, 0.0..=1.5).text("Over the top (m)"));
        ui.add(egui::Slider::new(&mut c.mantle_speed_mps, 0.1..=6.0).text("Mantle speed (m/s)"));
        ui.add(
            egui::Slider::new(&mut c.min_ledge_normal_y, 0.0..=1.0)
                .text("Least upward normal of a ledge"),
        );
    });
}

fn hold_label(hold: &Hold) -> String {
    format!(
        "body {} at ({:.1}, {:.1}, {:.1}) facing ({:+.2}, {:+.2}, {:+.2})",
        hold.body,
        hold.point.x,
        hold.point.y,
        hold.point.z,
        hold.normal.x,
        hold.normal.y,
        hold.normal.z
    )
}
